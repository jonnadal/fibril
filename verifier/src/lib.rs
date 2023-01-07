//! Fibril Verifier is a library for model checking Fibril systems.
//!
//! # Example
//!
//! ```toml
//! [dependencies]
//! fibril = { version = "0", features = ["serde"] }
//! serde = { version = "1", features = ["derive"] }
//!
//! [dev-dependencies]
//! fibril_verifier = "0"
//! ```
//!
//! ```rust
//! use fibril::*;
//! use fibril_verifier::*;
//!
//! #[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
//! enum Msg { Inc, Double, Value(u64) }
//!
//! let mut verifier = Verifier::new(|cfg| {
//!     let server = cfg.spawn(Fiber::new(|sdk| {
//!         let mut val = 0;
//!         loop {
//!             let (src, msg) = sdk.recv();
//!             match msg {
//!                 Msg::Inc => {
//!                     val += 1;
//!                     sdk.send(src, Msg::Value(val));
//!                 }
//!                 Msg::Double => {
//!                     val *= 2;
//!                     sdk.send(src, Msg::Value(val));
//!                 }
//!                 _ => sdk.exit(),
//!             }
//!         }
//!     }));
//!     cfg.spawn(Fiber::new(move |sdk| {
//!         sdk.send(server, Msg::Inc);
//!         let (_src, msg) = sdk.recv();
//!         assert_eq!(msg, Msg::Value(1)); // truth depends on race winner
//!     }));
//!     cfg.spawn(Fiber::new(move |sdk| {
//!         sdk.send(server, Msg::Double);
//!         let (_src, msg) = sdk.recv();
//!         assert_eq!(msg, Msg::Value(2)); // truth depends on race winner
//!     }));
//! });
//! let (msg, _minimal_trace) = verifier.assert_panic();
//! assert!(msg.contains("left == right"));
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
