use {
    fibril_core::Deadline,
    std::{collections::BTreeMap, time::Duration},
};

#[derive(Default)]
pub(crate) struct TimeContext {
    next_new_deadline: Deadline,
    pending_deadlines: BTreeMap<Deadline, Duration>,
}

impl TimeContext {
    pub(crate) fn new_deadline(&mut self, duration: Duration) -> Deadline {
        let deadline = self.next_new_deadline;
        self.pending_deadlines.insert(deadline, duration);
        self.next_new_deadline.id += 1;
        deadline
    }

    pub(crate) fn possibilities_for_deadline_elapsed(&self, deadline: Deadline) -> Vec<bool> {
        let mut possibilities = Vec::with_capacity(2);
        possibilities.push(true);
        if self.pending_deadlines.contains_key(&deadline) {
            possibilities.push(false);
        }
        possibilities
    }

    pub(crate) fn deadline_elapsed(&mut self, elapsed_deadline: Deadline) {
        let elapsed_duration = match self.pending_deadlines.get(&elapsed_deadline) {
            Some(duration) => duration,
            None => return,
        };
        let completed_deadlines: Vec<Deadline> = self
            .pending_deadlines
            .iter()
            .take_while(|(deadline, _)| deadline <= &&elapsed_deadline)
            .filter(|(_, duration)| duration <= &elapsed_duration)
            .map(|(deadline, _)| *deadline)
            .collect();
        for deadline in completed_deadlines {
            assert!(self.pending_deadlines.remove(&deadline).is_some());
        }
    }

    pub(crate) fn reset(&mut self) {
        self.next_new_deadline = Deadline::default();
        self.pending_deadlines.clear();
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn smoke_test() {
        let mut tc = TimeContext::default();

        // Case 1
        let a = tc.new_deadline(Duration::from_secs(5));
        let b = tc.new_deadline(Duration::from_secs(10));
        tc.deadline_elapsed(a);
        assert_eq!(tc.possibilities_for_deadline_elapsed(a), vec![true],);
        assert_eq!(tc.possibilities_for_deadline_elapsed(b), vec![true, false],);

        // Case 2
        let a = tc.new_deadline(Duration::from_secs(5));
        let b = tc.new_deadline(Duration::from_secs(10));
        tc.deadline_elapsed(b);
        assert_eq!(tc.possibilities_for_deadline_elapsed(a), vec![true],);
        assert_eq!(tc.possibilities_for_deadline_elapsed(b), vec![true],);

        // Case 3
        let a = tc.new_deadline(Duration::from_secs(10));
        let b = tc.new_deadline(Duration::from_secs(5));
        tc.deadline_elapsed(a);
        assert_eq!(tc.possibilities_for_deadline_elapsed(a), vec![true],);
        assert_eq!(tc.possibilities_for_deadline_elapsed(b), vec![true, false],);

        // Case 4
        let a = tc.new_deadline(Duration::from_secs(10));
        let b = tc.new_deadline(Duration::from_secs(5));
        tc.deadline_elapsed(b);
        assert_eq!(tc.possibilities_for_deadline_elapsed(a), vec![true, false],);
        assert_eq!(tc.possibilities_for_deadline_elapsed(b), vec![true],);
    }
}
