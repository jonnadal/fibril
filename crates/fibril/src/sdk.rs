use {
    corosensei::Yielder,
    fibril_core::{Command, Event, Id},
    std::collections::{BTreeMap, BTreeSet},
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

    pub fn recv_response_count(&self, count: usize, filter: impl Fn(Id, M) -> bool) {
        self.recv_btree_set(count, |src, msg| filter(src, msg).then_some(src));
    }

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
}
