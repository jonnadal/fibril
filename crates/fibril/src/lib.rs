//! Fibril is a library for implementing distributed systems with commmunicating fibers.
//!
//! # Usage
//!
//! Please see [the `fibril_verifier` docs](https://docs.rs/fibril_verifier/).
//!
//! # Features
//!
//! - `fibers`: Include support for
//! [fibers](https://en.wikipedia.org/wiki/Fiber_(computer_science)).
//! - `rt`: Include a UDP runtime.
//! - `serde_json`: Include a `new_with_serde_json` helper in the UDP runtime.

#![cfg_attr(all(doc, CHANNEL_NIGHTLY), feature(doc_auto_cfg))]
#![deny(unused_must_use)]
#![warn(rust_2018_idioms, unreachable_pub)]

#[cfg(feature = "fibers")]
mod fiber;
#[cfg(feature = "fibers")]
mod sdk;
#[cfg(feature = "rt")]
mod udp;

pub use fibril_core::Deadline;

#[cfg(feature = "fibers")]
pub use fiber::Fiber;

pub use fibril_core::Id;

#[cfg(feature = "fibers")]
pub use sdk::Sdk;

#[cfg(feature = "rt")]
pub use udp::UdpRuntime;
