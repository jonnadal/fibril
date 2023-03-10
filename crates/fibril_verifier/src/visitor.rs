use {
    crate::TraceRecord,
    std::sync::{Arc, Mutex},
};

pub trait Visitor<M> {
    fn on_maximal(&mut self, trace_records: &[TraceRecord<M>]);
}

impl<F, M> Visitor<M> for F
where
    F: FnMut(&[TraceRecord<M>]),
{
    fn on_maximal(&mut self, trace_records: &[TraceRecord<M>]) {
        self(trace_records);
    }
}

pub struct TraceRecordingVisitor<M>(Arc<Mutex<Vec<Vec<TraceRecord<M>>>>>);

impl<M> TraceRecordingVisitor<M> {
    pub fn new_with_replay() -> (Self, impl Fn() -> Vec<Vec<TraceRecord<M>>>)
    where
        M: Clone,
    {
        let visitor = TraceRecordingVisitor(Arc::new(Mutex::new(Vec::new())));
        let trace = Arc::clone(&visitor.0);
        let replay = move || trace.lock().unwrap().clone();
        (visitor, replay)
    }
}

impl<M> Visitor<M> for TraceRecordingVisitor<M>
where
    M: Clone,
{
    fn on_maximal(&mut self, trace_records: &[TraceRecord<M>]) {
        self.0.lock().unwrap().push(trace_records.to_vec());
    }
}
