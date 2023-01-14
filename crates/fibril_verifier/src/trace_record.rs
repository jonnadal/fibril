use {
    fibril_core::{Command, Event, Id},
    std::fmt::{Debug, Display, Formatter},
    vector_clock::VectorClock,
};

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct TraceRecord<M> {
    pub(crate) event: Event<M>,
    pub(crate) event_clock: VectorClock,
    pub(crate) id: Id,
    pub(crate) command: Command<M>,
    pub(crate) clock: VectorClock,
}

impl<M> Display for TraceRecord<M>
where
    M: Debug,
{
    fn fmt(&self, formatter: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
        formatter.write_fmt(format_args!(
            "{:?}@{} → {} → {:?}@{}",
            self.event, self.event_clock, self.id, self.command, self.clock
        ))
    }
}
