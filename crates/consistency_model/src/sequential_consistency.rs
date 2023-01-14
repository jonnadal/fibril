use crate::{ConsistencyTester, SequentialSpec};
use std::collections::{btree_map, BTreeMap, VecDeque};
use std::fmt::Debug;

/// This tester captures a potentially concurrent history of operations and
/// validates that it adheres to a [`SequentialSpec`] based on the
/// [sequential consistency] model. This model requires that operations be
/// applied atomically and that operations within a thread are sequential
/// (they have a total order within the thread).
///
/// If you're not sure whether to pick this or [`LinearizabilityTester`], favor
/// `LinearizabilityTester`.
///
/// # Sequential Consistency
///
/// Unlike with [linearizability], there is no intrinsic order of operations across threads, even
/// if they are fully sequenced in "real-time" (defined more precisely below). Anomalies are
/// therefore possible as threads do not necessarily agree on viable orders of operations.  For
/// example, the later read by Thread 2 in this diagram is allowed to return the value prior to
/// Thread 1's write even though the operations are seemingly not concurrent:
///
/// ```text
///           -----------Time------------------------------>
/// Thread 1: [write invoked... and returns]
/// Thread 2:                                 [read invoked... and returns]
/// ```
///
/// While "real time" is a common way to phrase an implicit total ordering on non-concurrent events
/// spanning threads, a more precise way to think about this is that prior to Thread 2 starting its
/// read, Thread 1 is capable of communicating with Thread 2 indicating that the write finished.
/// This perspective avoids introducing the notion of a shared global time, which is often a
/// misleading perspective when it comes to distributed systems.
///
/// The [`SequentialSpec`] will imply additional ordering constraints based on semantics specific
/// to each operation. For example, a value cannot be popped off a stack before it is pushed. It is
/// then the responsibility of the checker to establish whether a valid total ordering of events
/// exists under these constraints.
///
/// See also: [`LinearizabilityTester`].
///
/// [linearizability]: https://en.wikipedia.org/wiki/Linearizability
/// [sequential consistency]: https://en.wikipedia.org/wiki/Sequential_consistency
/// [`LinearizabilityTester`]: crate::LinearizabilityTester
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[allow(clippy::type_complexity)]
pub struct SequentialConsistencyTester<ThreadId, RefObj: SequentialSpec> {
    init_ref_obj: RefObj,
    history_by_thread: BTreeMap<ThreadId, VecDeque<(RefObj::Op, RefObj::Ret)>>,
    in_flight_by_thread: BTreeMap<ThreadId, RefObj::Op>,
    is_valid_history: bool,
}

#[allow(clippy::len_without_is_empty)] // no use case for an emptiness check
impl<T: Ord, RefObj: SequentialSpec> SequentialConsistencyTester<T, RefObj> {
    /// Constructs a [`SequentialConsistencyTester`].
    pub fn new(init_ref_obj: RefObj) -> Self {
        Self {
            init_ref_obj,
            history_by_thread: Default::default(),
            in_flight_by_thread: Default::default(),
            is_valid_history: true,
        }
    }

    /// Indicates the aggregate number of operations completed or in flight
    /// across all threads.
    pub fn len(&self) -> usize {
        let mut len = self.in_flight_by_thread.len();
        for history in self.history_by_thread.values() {
            len += history.len();
        }
        len
    }
}

impl<T, RefObj> ConsistencyTester<T, RefObj> for SequentialConsistencyTester<T, RefObj>
where
    T: Copy + Debug + Ord,
    RefObj: Clone + SequentialSpec,
    RefObj::Op: Clone + Debug,
    RefObj::Ret: Clone + Debug + PartialEq,
{
    /// Indicates that a thread invoked an operation. Returns `Ok(...)` if the
    /// history is valid, even if it is not sequentially consistent.
    ///
    /// See [`SequentialConsistencyTester::serialized_history`].
    fn on_invoke(&mut self, thread_id: T, op: RefObj::Op) -> Result<&mut Self, String> {
        if !self.is_valid_history {
            return Err("Earlier history was invalid.".to_string());
        }
        let in_flight_elem = self.in_flight_by_thread.entry(thread_id);
        if let btree_map::Entry::Occupied(occupied_op_entry) = in_flight_elem {
            self.is_valid_history = false;
            return Err(format!(
                    "Thread already has an operation in flight. thread_id={:?}, op={:?}, history_by_thread={:?}",
                    thread_id, occupied_op_entry.get(), self.history_by_thread));
        };
        in_flight_elem.or_insert(op);
        self.history_by_thread
            .entry(thread_id)
            .or_insert_with(VecDeque::new); // `serialize` requires entry
        Ok(self)
    }

    /// Indicates that a thread's earlier operation invocation returned. Returns
    /// `Ok(...)` if the history is valid, even if it is not sequentially
    /// consistent.
    ///
    /// See [`SequentialConsistencyTester::serialized_history`].
    fn on_return(&mut self, thread_id: T, ret: RefObj::Ret) -> Result<&mut Self, String> {
        if !self.is_valid_history {
            return Err("Earlier history was invalid.".to_string());
        }
        let op = match self.in_flight_by_thread.remove(&thread_id) {
            None => {
                self.is_valid_history = false;
                return Err(format!(
                    "There is no in-flight invocation for this thread ID. \
                     thread_id={:?}, unexpected_return={:?}, history={:?}",
                    thread_id,
                    ret,
                    self.history_by_thread
                        .entry(thread_id)
                        .or_insert_with(VecDeque::new)
                ));
            }
            Some(op) => op,
        };
        self.history_by_thread
            .entry(thread_id)
            .or_insert_with(VecDeque::new)
            .push_back((op, ret));
        Ok(self)
    }

    /// Indicates whether the recorded history is sequentially consistent.
    fn is_consistent(&self) -> bool {
        self.serialized_history().is_some()
    }
}

impl<T, RefObj> SequentialConsistencyTester<T, RefObj>
where
    T: Copy + Debug + Ord,
    RefObj: Clone + SequentialSpec,
    RefObj::Op: Clone + Debug,
    RefObj::Ret: Clone + Debug + PartialEq,
{
    /// Attempts to serialize the recorded partially ordered operation history
    /// into a total order that is consistent with a reference object's
    /// operational semantics.
    pub fn serialized_history(&self) -> Option<Vec<(RefObj::Op, RefObj::Ret)>>
    where
        RefObj: Clone,
        RefObj::Op: Clone,
        RefObj::Ret: Clone,
    {
        if !self.is_valid_history {
            return None;
        }
        Self::serialize(
            Vec::new(),
            &self.init_ref_obj,
            &self.history_by_thread,
            &self.in_flight_by_thread,
        )
    }

    #[allow(clippy::type_complexity)]
    fn serialize(
        valid_history: Vec<(RefObj::Op, RefObj::Ret)>, // total order
        ref_obj: &RefObj,
        remaining_history_by_thread: &BTreeMap<T, VecDeque<(RefObj::Op, RefObj::Ret)>>, // partial order
        in_flight_by_thread: &BTreeMap<T, RefObj::Op>,
    ) -> Option<Vec<(RefObj::Op, RefObj::Ret)>>
    where
        RefObj: Clone,
        RefObj::Op: Clone,
        RefObj::Ret: Clone,
    {
        // Return collected total order when there is no remaining partial order to interleave.
        let done = remaining_history_by_thread
            .iter()
            .all(|(_id, h)| h.is_empty());
        if done {
            return Some(valid_history);
        }

        // Otherwise try remaining interleavings.
        for (thread_id, remaining_history) in remaining_history_by_thread.iter() {
            let mut remaining_history_by_thread =
                std::borrow::Cow::Borrowed(remaining_history_by_thread);
            let mut in_flight_by_thread = std::borrow::Cow::Borrowed(in_flight_by_thread);
            let (ref_obj, valid_history) = if remaining_history.is_empty() {
                // Case 1: No remaining history to interleave. Maybe in-flight.
                if !in_flight_by_thread.contains_key(thread_id) {
                    continue;
                }
                let op = in_flight_by_thread.to_mut().remove(thread_id).unwrap(); // `contains_key` above
                let mut ref_obj = ref_obj.clone();
                let ret = ref_obj.invoke(&op);
                let mut valid_history = valid_history.clone();
                valid_history.push((op, ret));
                (ref_obj, valid_history)
            } else {
                // Case 2: Has remaining history to interleave.
                let (op, ret) = remaining_history_by_thread
                    .to_mut()
                    .get_mut(thread_id)
                    .unwrap() // iterator returned this thread ID
                    .pop_front()
                    .unwrap(); // `!is_empty()` above
                let mut ref_obj = ref_obj.clone();
                if !ref_obj.is_valid_step(&op, &ret) {
                    continue;
                }
                let mut valid_history = valid_history.clone();
                valid_history.push((op, ret));
                (ref_obj, valid_history)
            };
            if let Some(valid_history) = Self::serialize(
                valid_history,
                &ref_obj,
                &remaining_history_by_thread,
                &in_flight_by_thread,
            ) {
                return Some(valid_history);
            }
        }
        None
    }
}

impl<T: Ord, RefObj> Default for SequentialConsistencyTester<T, RefObj>
where
    RefObj: Default + SequentialSpec,
{
    fn default() -> Self {
        Self::new(RefObj::default())
    }
}

#[cfg(feature = "serde")]
impl<T, RefObj> serde::Serialize for SequentialConsistencyTester<T, RefObj>
where
    RefObj: serde::Serialize + SequentialSpec,
    RefObj::Op: serde::Serialize,
    RefObj::Ret: serde::Serialize,
    T: Ord + serde::Serialize,
{
    fn serialize<Ser: serde::Serializer>(&self, ser: Ser) -> Result<Ser::Ok, Ser::Error> {
        use serde::ser::SerializeStruct;
        let mut out = ser.serialize_struct("SequentialConsistencyTester", 4)?;
        out.serialize_field("init_ref_obj", &self.init_ref_obj)?;
        out.serialize_field("history_by_thread", &self.history_by_thread)?;
        out.serialize_field("in_flight_by_thread", &self.in_flight_by_thread)?;
        out.serialize_field("is_valid_history", &self.is_valid_history)?;
        out.end()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::*;

    #[test]
    fn rejects_invalid_history() -> Result<(), String> {
        assert_eq!(
            SequentialConsistencyTester::new(Register::new('A'))
                .on_invoke(99, RegisterOp::Write('B'))?
                .on_invoke(99, RegisterOp::Write('C')),
            Err("Thread already has an operation in flight. thread_id=99, op=Write('B'), history_by_thread={99: []}".to_string()));
        assert_eq!(
            SequentialConsistencyTester::new(Register::new('A'))
                .on_invret(99, RegisterOp::Write('B'), RegisterRet::WriteOk)?
                .on_invret(99, RegisterOp::Write('C'), RegisterRet::WriteOk)?
                .on_return(99, RegisterRet::WriteOk),
            Err("There is no in-flight invocation for this thread ID. \
                 thread_id=99, \
                 unexpected_return=WriteOk, \
                 history=[(Write('B'), WriteOk), (Write('C'), WriteOk)]"
                .to_string())
        );
        Ok(())
    }

    #[test]
    fn identifies_serializable_register_history() -> Result<(), String> {
        assert_eq!(
            SequentialConsistencyTester::new(Register::new('A'))
                .on_invoke(0, RegisterOp::Write('B'))?
                .on_invret(1, RegisterOp::Read, RegisterRet::ReadOk('A'))?
                .serialized_history(),
            Some(vec![(RegisterOp::Read, RegisterRet::ReadOk('A')),])
        );
        assert_eq!(
            SequentialConsistencyTester::new(Register::new('A'))
                .on_invret(0, RegisterOp::Read, RegisterRet::ReadOk('B'))?
                .on_invoke(1, RegisterOp::Write('B'))?
                .serialized_history(),
            Some(vec![
                (RegisterOp::Write('B'), RegisterRet::WriteOk),
                (RegisterOp::Read, RegisterRet::ReadOk('B')),
            ])
        );
        Ok(())
    }

    #[test]
    fn identifies_unserializable_register_history() -> Result<(), String> {
        assert_eq!(
            SequentialConsistencyTester::new(Register::new('A'))
                .on_invret(0, RegisterOp::Read, RegisterRet::ReadOk('B'))?
                .serialized_history(),
            None
        );
        Ok(())
    }

    #[test]
    fn identifies_serializable_vec_history() -> Result<(), String> {
        assert_eq!(
            SequentialConsistencyTester::new(Vec::new())
                .on_invoke(0, VecOp::Push(10))?
                .serialized_history(),
            Some(vec![])
        );
        assert_eq!(
            SequentialConsistencyTester::new(Vec::new())
                .on_invoke(0, VecOp::Push(10))?
                .on_invret(1, VecOp::Pop, VecRet::PopOk(None))?
                .serialized_history(),
            Some(vec![(VecOp::Pop, VecRet::PopOk(None)),])
        );
        assert_eq!(
            SequentialConsistencyTester::new(Vec::new())
                .on_invret(1, VecOp::Pop, VecRet::PopOk(Some(10)))?
                .on_invret(0, VecOp::Push(10), VecRet::PushOk)?
                .on_invret(0, VecOp::Pop, VecRet::PopOk(Some(20)))?
                .on_invoke(0, VecOp::Push(30))?
                .on_invret(1, VecOp::Push(20), VecRet::PushOk)?
                .on_invret(1, VecOp::Pop, VecRet::PopOk(None))?
                .serialized_history(),
            Some(vec![
                (VecOp::Push(10), VecRet::PushOk),
                (VecOp::Pop, VecRet::PopOk(Some(10))),
                (VecOp::Push(20), VecRet::PushOk),
                (VecOp::Pop, VecRet::PopOk(Some(20))),
                (VecOp::Pop, VecRet::PopOk(None)),
            ])
        );
        Ok(())
    }

    #[test]
    fn identifies_unserializable_vec_history() -> Result<(), String> {
        assert_eq!(
            SequentialConsistencyTester::new(Vec::new())
                .on_invret(0, VecOp::Push(10), VecRet::PushOk)?
                .on_invoke(0, VecOp::Push(20))?
                .on_invret(1, VecOp::Len, VecRet::LenOk(2))?
                .on_invret(1, VecOp::Pop, VecRet::PopOk(Some(10)))?
                .on_invret(1, VecOp::Pop, VecRet::PopOk(Some(20)))?
                .serialized_history(),
            None
        );
        Ok(())
    }
}
