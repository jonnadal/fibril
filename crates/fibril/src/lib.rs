//! Fibril is a library for implementing distributed systems with commmunicating fibers.

#![deny(unused_must_use)]
#![warn(rust_2018_idioms, unreachable_pub)]

#[cfg(feature = "fibers")]
use {
    corosensei::{stack::DefaultStack, ScopedCoroutine, Yielder},
    fibril_core::{Command, Event},
};

#[cfg(feature = "fibers")]
mod fiber;
#[cfg(feature = "fibers")]
mod sdk;
#[cfg(feature = "rt")]
mod udp;

#[cfg(feature = "fibers")]
pub struct Fiber<'a, M>(ScopedCoroutine<'a, Event<M>, Command<M>, (), DefaultStack>);

pub use fibril_core::Id;

#[cfg(feature = "fibers")]
pub struct Sdk<'a, M>(&'a Yielder<Event<M>, Command<M>>, Id);

#[cfg(feature = "rt")]
pub use udp::UdpRuntime;
