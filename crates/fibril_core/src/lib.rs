//! This module specifies the core types for the [Fibril](https://docs.rs/fibril/) library.
//!
//! # Usage
//!
//! Please see the [the `fibril_verifier` docs](https://docs.rs/fibril_verifier/).
//!
//! # Features
//!
//! - `serde`: Implement `Serialize` and `Deserialize` where applicable.

#![cfg_attr(all(doc, CHANNEL_NIGHTLY), feature(doc_auto_cfg))]
// Support using Fibril core without the standard library.
#![cfg_attr(not(feature = "std"), no_std)]
#![deny(unused_must_use)]
#![warn(rust_2018_idioms, unreachable_pub)]

mod id;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[non_exhaustive]
pub enum Command<M> {
    Exit,
    Expect(String),
    ExpectationMet(Expectation),
    Panic(String),
    Recv,
    Send(Id, M),
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[non_exhaustive]
pub enum Event<M> {
    ExpectOk(Expectation),
    ExpectationMetOk,
    RecvOk(Id, M),
    SendOk,
    SpawnOk(Id),
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct Expectation(String);
impl Expectation {
    pub fn new(description: String) -> Self {
        Expectation(description)
    }

    pub fn description(&self) -> &String {
        &self.0
    }
}

pub use id::Id;

pub trait Step<M> {
    fn step(&mut self, event: Event<M>) -> Command<M>;
}
impl<M, F> Step<M> for F
where
    F: FnMut(Event<M>) -> Command<M>,
{
    fn step(&mut self, event: Event<M>) -> Command<M> {
        self(event)
    }
}
