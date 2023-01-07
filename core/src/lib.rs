//! This module specifies the core types for the Fibril library.

#![deny(unused_must_use)]
#![warn(rust_2018_idioms, unreachable_pub)]

mod id;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[non_exhaustive]
pub enum Command<M> {
    Exit,
    Panic(String),
    Recv,
    Send(Id, M),
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[non_exhaustive]
pub enum Event<M> {
    RecvOk(Id, M),
    SendOk,
    SpawnOk(Id),
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct Id(usize);

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
