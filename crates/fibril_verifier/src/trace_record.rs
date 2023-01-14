use {
    crate::TraceRecord,
    std::fmt::{Debug, Display, Formatter},
};

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
