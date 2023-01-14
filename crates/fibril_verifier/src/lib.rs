//! Fibril Verifier is a library for model checking [Fibril](https://docs.rs/fibril/) systems.
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

pub use trace_record::TraceRecord;

pub use visitor::TraceRecordingVisitor;

pub use verifier::RunResult;

pub use verifier::Verifier;

pub use verifier::VerifierConfig;

pub use visitor::Visitor;
