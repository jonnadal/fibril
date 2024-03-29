use {
    corosensei::Yielder,
    fibril_core::{Command, Deadline, Event, Expectation, Id},
    std::{
        collections::{BTreeMap, BTreeSet},
        fmt::{Debug, Formatter},
        time::Duration,
    },
};

pub struct Sdk<'a, M>(pub(crate) &'a Yielder<Event<M>, Command<M>>, pub(crate) Id);

impl<'a, M> Sdk<'a, M> {
    pub fn deadline(&self, duration: Duration) -> Deadline {
        match self.0.suspend(Command::Deadline(duration)) {
            Event::DeadlineOk(deadline) => deadline,
            _ => unreachable!(),
        }
    }

    pub fn deadline_elapsed(&self, deadline: Deadline) -> bool {
        match self.0.suspend(Command::DeadlineElapsed(deadline)) {
            Event::DeadlineElapsedOk(truth) => truth,
            _ => unreachable!(),
        }
    }

    pub fn exit(&self) -> ! {
        self.0.suspend(Command::Exit);
        unreachable!();
    }

    #[must_use]
    pub fn expect(&self, description: impl ToString) -> Expectation {
        let description = description.to_string();
        match self.0.suspend(Command::Expect(description)) {
            Event::ExpectOk(expectation) => expectation,
            _ => unreachable!(),
        }
    }

    pub fn expectation_met(&self, expectation: Expectation) {
        match self.0.suspend(Command::ExpectationMet(expectation)) {
            Event::ExpectationMetOk => {}
            _ => unreachable!(),
        }
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

    pub fn recv_btree_map<K, V>(
        &self,
        count: usize,
        filter_map: impl Fn(Id, M) -> Option<(K, V)>,
    ) -> BTreeMap<K, V>
    where
        K: Ord,
    {
        let mut collected = BTreeMap::new();
        while collected.len() < count {
            let (src, msg) = self.recv();
            if let Some((key, value)) = filter_map(src, msg) {
                collected.insert(key, value);
            }
        }
        collected
    }

    pub fn recv_btree_set<V>(
        &self,
        count: usize,
        filter_map: impl Fn(Id, M) -> Option<V>,
    ) -> BTreeSet<V>
    where
        V: Ord,
    {
        let mut collected = BTreeSet::new();
        while collected.len() < count {
            let (src, msg) = self.recv();
            if let Some(value) = filter_map(src, msg) {
                collected.insert(value);
            }
        }
        collected
    }

    /// This is a helper based on [`Sdk::recv_btree_set`] for the case where an actor needs to
    /// wait for `count` messages from distinct recipients (e.g. when awaiting
    /// [quorum](https://en.wikipedia.org/wiki/Quorum_%28distributed_computing%29)).
    ///
    /// `count` will match the number of distinct [`Id`]s for which the `filter` returned
    /// `true` (i.e. accepted messages from the same [`Id`] are only counted once).
    pub fn recv_response_count(&self, count: usize, filter: impl Fn(Id, M) -> bool) {
        self.recv_btree_set(count, |src, msg| filter(src, msg).then_some(src));
    }

    /// This is a helper based on [`Sdk::recv_btree_map`] for the case where an actor needs to
    /// wait for `count` messages from distinct recipients (e.g. when awaiting
    /// [quorum](https://en.wikipedia.org/wiki/Quorum_%28distributed_computing%29)).
    ///
    /// `count` will match the number of distinct [`Id`]s for which the `filter_map` returned
    /// `Some(...)` (i.e. accepted messages from the same [`Id`] are only counted once).
    pub fn recv_responses<V>(
        &self,
        count: usize,
        filter_map: impl Fn(Id, M) -> Option<V>,
    ) -> BTreeMap<Id, V> {
        self.recv_btree_map(count, |src, msg| filter_map(src, msg).map(|v| (src, v)))
    }

    pub fn send(&self, dst: Id, m: M) {
        let input = self.0.suspend(Command::Send(dst, m));
        assert!(matches!(input, Event::SendOk));
    }

    pub fn send_broadcast(&self, dst: impl IntoIterator<Item = Id>, m: &M)
    where
        M: Clone,
    {
        for dst in dst {
            self.send(dst, m.clone());
        }
    }

    pub fn sleep_until(&self, deadline: Deadline) {
        match self.0.suspend(Command::SleepUntil(deadline)) {
            Event::SleepUntilOk => (),
            _ => unreachable!(),
        }
    }
}

impl<'a, M> Debug for Sdk<'a, M> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Sdk@{}", self.1)
    }
}
