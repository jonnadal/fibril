//! Fibril Verifier is a library for model checking Fibril systems.
//!
//! # Example
//!
//! ```toml
//! [dependencies]
//! fibril = { version = "0", features = ["fibers", "rt", "serde_json"] }
//! serde = { version = "1", features = ["derive"] }
//!
//! [dev-dependencies]
//! fibril_verifier = "0"
//! ```
//!
//! ```rust
//! use fibril::*;
//!
//! #[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
//! enum Msg { Inc, Double, Value(u64) }
//!
//! fn server(sdk: Sdk<Msg>) {
//!     let mut val = 0;
//!     loop {
//!         let (src, msg) = sdk.recv();
//!         match msg {
//!             Msg::Inc => {
//!                 val += 1;
//!                 sdk.send(src, Msg::Value(val));
//!             }
//!             Msg::Double => {
//!                 val *= 2;
//!                 sdk.send(src, Msg::Value(val));
//!             }
//!             _ => sdk.exit(),
//!         }
//!     }
//! }
//!
//! // In the binary, bind the server to a UDP socket.
//! fn run_server_over_udp() {
//!     use std::net::Ipv4Addr;
//!     let mut rt = UdpRuntime::new_with_serde_json()
//!         .ipv4(Ipv4Addr::LOCALHOST)
//!         .port_fn(|n| 3000 + n);
//!     rt.spawn(|| Fiber::new(server));
//!     rt.join().unwrap();
//! }
//!
//! // In the tests, the model checker will attempt to find a "trace" (sequence of steps) that
//! // causes a panic.
//! #[cfg(test)]
//! #[test]
//! # fn hack_so_next_fn_is_compiled_in_rustdoc() {}
//! fn may_panic() {
//!     use fibril_verifier::*;
//!     let mut verifier = Verifier::new(|cfg| {
//!         let server = cfg.spawn(Fiber::new(server));
//!         cfg.spawn(Fiber::new(move |sdk| {
//!             sdk.send(server, Msg::Inc);
//!             let (_src, msg) = sdk.recv();
//!             assert_eq!(msg, Msg::Value(1)); // truth depends on race winner
//!         }));
//!         cfg.spawn(Fiber::new(move |sdk| {
//!             sdk.send(server, Msg::Double);
//!             let (_src, msg) = sdk.recv();
//!             assert_eq!(msg, Msg::Value(2)); // truth depends on race winner
//!         }));
//!     });
//!     // TIP: alternatively use verifier.assert_no_panic() to fail the test.
//!     let (msg, _minimal_trace) = verifier.assert_panic();
//!     assert!(msg.contains("left == right"));
//! }
//! # may_panic();
//! ```
//!
//! And here is an example of how to interact with the server using
//! [netcat](https://en.wikipedia.org/wiki/Netcat).
//!
//! ```sh
//! # send STDOUT to disk for clarity as replies are not newline delimited
//! $ nc -u 127.0.0.1 3000 > /tmp/replies
//! "Inc"
//! "Inc"
//! "Double"
//! ^C
//!
//! $ cat /tmp/replies
//! {"Value":1}{"Value":2}{"Value":4}
//! ```

#![deny(unused_must_use)]
#![warn(rust_2018_idioms, unreachable_pub)]

mod trace_record;
mod trace_tree;
mod verifier;
mod visitor;

use {
    fibril_core::{Command, Event, Id, Step},
    std::sync::{Arc, Mutex},
    vector_clock::VectorClock,
};

#[macro_export]
macro_rules! assert_trace {
    // Case 1: No expected records specified.
    [$records:expr $(,)?] => {
        if !$records.is_empty() {
            println!("Missing some records:");
            for r in $records.iter() {
                println!("\"{}\",", format!("{}", r).escape_debug().to_string());
            }
            panic!("^");
        }
    };
    // Case 2: Expected record(s) specified. Requires recursion.
    [$records:expr, $str:tt, $($rest:tt)*] => {
        fibril_verifier::assert_trace_![0 => $records, $str, $($rest)*];
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! assert_trace_ {
    // Base case: only one string to assert.
    ($i:expr => $records:expr, $str:tt $(,)?) => (
        assert_eq!($records[$i].to_string().as_str(), $str, "at [{}]", $i);
        if $i + 1 < $records.len() {
            println!("Missing some records:");
            for r in $records.iter().skip($i + 1) {
                println!("\"{}\",", format!("{}", r).escape_debug().to_string());
            }
            panic!("^");
        }
    );
    // Inductive case: assert and recurse.
    ($i:expr => $records:expr, $str:tt, $($rest:tt)*) => (
        assert_eq!($records[$i].to_string().as_str(), $str, "at [{}]", $i);
        fibril_verifier::assert_trace_!($i + 1 => $records, $($rest)*);
    );
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct TraceRecord<M> {
    event: Event<M>,
    event_clock: VectorClock,
    id: Id,
    command: Command<M>,
    clock: VectorClock,
}

pub struct TraceRecordingVisitor<M>(Arc<Mutex<Vec<Vec<TraceRecord<M>>>>>);

#[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[non_exhaustive]
pub enum RunResult<M> {
    Complete,
    Incomplete,
    Panic {
        message: String,
        minimal_trace: Vec<TraceRecord<M>>,
    },
}

/// A runtime that can verify safety properties observable by distributed [`Step`] instances.
///
/// # Purpose
///
/// In the presence of nondeterminism, most runtimes execute one "schedule" of events. For
/// instance, if clients _c<sub>A</sub>_ and _c<sub>B</sub>_ send two racing messages `A` and
/// `B` to server _s_, then a typical scheduler will either exercise the case where message `A`
/// lands before message `B` or the case where `B` lands before `A`. This runtime will exercise
/// both.
///
/// By exercising all anticipated nondeterministic outcomes (even seemingly unlikely ones like
/// unbounded message delivery delay), we can infer that any assertions that pass during
/// verification should also hold during real-world scenarios.
///
/// # Internal Implementation Details
///
/// ```text
/// You can safely ignore this section if you only want to use the library. It is only
/// included as documentation for library contributors.
/// ```
///
/// The implementation is based on my understanding of _[Source Sets: A Foundation for Optimal
/// Dynamic Partial Order Reduction](https://dl.acm.org/doi/10.1145/3073408)_ (and feedback is very
/// much appreciated if I got this wrong):
///
/// 1. Each sequential behavior implementing [`Step`] is paired with a _wakeup tree_.  Each tree
///    persists the sequenced events experienced by that actor for all visited system traces.
/// 2. When evaluating whether to schedule a potential [`Event`] for an actor, the scheduler
///    first checks that actor's wakeup tree. It only schedules events not yet in the tree.
/// 3. Conversely, when an actor triggers a [`Command`] that generates an [`Event`], the event is
///    added to a queue for the recipient actor.
/// 4. The checker primes the queue by spawning actors and continues until all queues are
///    empty.
///
/// Additional notes:
///
/// 1. I believe the algorithm only relies on recording events corresponding with nondeterministic
///    commands (such as [`Event::RecvOk`] for [`Command::Recv`]) in the wakeup trees. For
///    debuggability the current implementation also records events corresponding with
///    deterministic commands (such as [`Event::SendOk`] for [`Command::Send`]).
/// 2. The implementation could persist event _fingerprints_ (digests) rather than the specific
///    events to reduce memory consumption at the cost of added CPU load. Whether this is
///    worthwhile remains to be seen.
pub struct Verifier<M> {
    actors: Vec<verifier::Actor<M>>,
    #[allow(clippy::type_complexity)]
    cfg_fn: Box<dyn Fn(&mut VerifierConfig<M>)>,
    next_prefix: Vec<(Id, Event<M>, VectorClock)>,
    trace_records: Vec<TraceRecord<M>>,
    visitors: Vec<Box<dyn Visitor<M>>>,
}

pub struct VerifierConfig<M> {
    behaviors: Vec<Box<dyn Step<M>>>,
}

pub trait Visitor<M> {
    fn on_maximal(&mut self, trace_records: &[TraceRecord<M>]);
}
