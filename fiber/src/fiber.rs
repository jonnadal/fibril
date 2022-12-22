use {
    crate::{Fiber, Sdk},
    corosensei::{CoroutineResult, ScopedCoroutine},
    fibril_core::{Command, Event, Step},
};

#[cfg(feature = "tracing")]
use tracing::{event, span, Level};

impl<'a, M> Fiber<'a, M> {
    pub fn new(behavior: impl FnOnce(Sdk<'_, M>) + 'a) -> Self {
        #[cfg(feature = "tracing")]
        let span = span!(Level::TRACE, "fiber");

        Self(
            ScopedCoroutine::new(move |yielder, spawn_ok| {
                let id = match spawn_ok {
                    Event::SpawnOk(id) => id,
                    _ => unreachable!(),
                };

                #[cfg(feature = "tracing")]
                event!(Level::TRACE, "fiber id: {}", id);

                behavior(Sdk(yielder, id))
            }),
            #[cfg(feature = "tracing")]
            span,
        )
    }
}
impl<M> Step<M> for Fiber<'_, M> {
    fn step(&mut self, event: Event<M>) -> Command<M> {
        #[cfg(feature = "tracing")]
        let span = span!(Level::TRACE, "step");
        #[cfg(feature = "tracing")]
        let _enter = span.enter();

        match self.0.resume(event) {
            CoroutineResult::Yield(command) => {
                #[cfg(feature = "tracing")]
                event!(Level::TRACE, "yield");
                command
            }
            CoroutineResult::Return(()) => {
                #[cfg(feature = "tracing")]
                event!(Level::TRACE, "return");
                Command::Exit
            }
        }
    }
}
