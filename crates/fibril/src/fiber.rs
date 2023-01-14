use {
    crate::{Fiber, Sdk},
    corosensei::{CoroutineResult, ScopedCoroutine},
    fibril_core::{Command, Event, Step},
};

impl<'a, M> Fiber<'a, M> {
    pub fn new(behavior: impl FnOnce(Sdk<'_, M>) + 'a) -> Self {
        Self(ScopedCoroutine::new(move |yielder, spawn_ok| {
            let id = match spawn_ok {
                Event::SpawnOk(id) => id,
                _ => unreachable!(),
            };
            behavior(Sdk(yielder, id))
        }))
    }
}
impl<M> Step<M> for Fiber<'_, M> {
    fn step(&mut self, event: Event<M>) -> Command<M> {
        match self.0.resume(event) {
            CoroutineResult::Yield(command) => command,
            CoroutineResult::Return(()) => Command::Exit,
        }
    }
}
