//! Fibril is a library for implementing distributed systems with commmunicating fibers.

#![deny(unused_must_use)]
#![warn(rust_2018_idioms, unreachable_pub)]

use {
    corosensei::{stack::DefaultStack, ScopedCoroutine, Yielder},
    fibril_core::{Command, Event},
};

mod fiber;
mod sdk;

pub struct Fiber<'a, M>(ScopedCoroutine<'a, Event<M>, Command<M>, (), DefaultStack>);

pub use fibril_core::Id;

pub struct Sdk<'a, M>(&'a Yielder<Event<M>, Command<M>>, Id);
