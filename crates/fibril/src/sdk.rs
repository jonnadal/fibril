use {
    corosensei::Yielder,
    fibril_core::{Command, Event, Id},
};

pub struct Sdk<'a, M>(pub(crate) &'a Yielder<Event<M>, Command<M>>, pub(crate) Id);

impl<'a, M> Sdk<'a, M> {
    pub fn exit(&self) -> ! {
        self.0.suspend(Command::Exit);
        unreachable!();
    }
    pub fn id(&self) -> Id {
        self.1
    }
    pub fn recv(&self) -> (Id, M) {
        match self.0.suspend(Command::Recv) {
            Event::RecvOk(src, m) => (src, m),
            _ => unreachable!(),
        }
    }
    pub fn send(&self, dst: Id, m: M) {
        let input = self.0.suspend(Command::Send(dst, m));
        assert!(matches!(input, Event::SendOk));
    }
}
