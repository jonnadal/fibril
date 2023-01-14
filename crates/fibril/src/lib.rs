//! Fibril is a library for implementing distributed systems with commmunicating fibers.
//!
//! # Usage
//!
//! Please see [the `fibril_verifier` docs](https://docs.rs/fibril_verifier/).

#![cfg_attr(all(doc, CHANNEL_NIGHTLY), feature(doc_auto_cfg))]
#![deny(unused_must_use)]
#![warn(rust_2018_idioms, unreachable_pub)]

#[cfg(feature = "fibers")]
mod fiber;
#[cfg(feature = "fibers")]
mod sdk;
#[cfg(feature = "rt")]
mod udp;

#[cfg(feature = "fibers")]
pub use fiber::Fiber;

pub use fibril_core::Id;

#[cfg(feature = "fibers")]
pub use sdk::Sdk;

#[cfg(feature = "rt")]
pub use udp::UdpRuntime;
