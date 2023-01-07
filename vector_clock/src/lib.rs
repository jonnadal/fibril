use std::cmp::{max, Ordering};
use std::fmt::{self, Display, Formatter};
use std::hash::{Hash, Hasher};

/// A [vector clock](https://en.wikipedia.org/wiki/Vector_clock), which provides a partial causal
/// order on events in a distributed sytem.
#[derive(Clone, Debug, Default, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct VectorClock(Vec<u32>);

/// Creates a [`VectorClock`] containing the specified elements.
///
/// # Example
/// ```
/// use vector_clock::vclock;
///
/// let x = vclock![];
/// let y = vclock![42, 0, 1];
/// ```
#[macro_export]
macro_rules! vclock {
    () => (
        $crate::VectorClock::new()
    );
    ($($x:expr),+ $(,)?) => (
        $crate::VectorClock::from(vec![$($x),+])
    );
}

impl VectorClock {
    pub fn increment(&mut self, index: usize) {
        if index >= self.0.len() {
            self.0.resize(1 + index, 0);
        }
        self.0[index] += 1;
    }

    pub fn merge_in(&mut self, other: &Self) {
        if other.0.len() > self.0.len() {
            self.0.resize(other.0.len(), 0);
        }
        for i in 0..other.0.len() {
            self.0[i] = max(self.0[i], other.0[i]);
        }
    }

    pub fn new() -> Self {
        VectorClock(Vec::new())
    }

    pub fn new_with_len(len: usize) -> Self {
        VectorClock(vec![0; len])
    }

    pub fn reset(&mut self) {
        for element in self.0.iter_mut() {
            *element = 0;
        }
    }
}

impl Display for VectorClock {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), fmt::Error> {
        write!(f, "<")?;
        let mut iter = self.0.iter();
        if let Some(mut next) = iter.next() {
            loop {
                write!(f, "{}", next)?;
                next = match iter.next() {
                    None => break,
                    Some(next) => {
                        write!(f, " ")?;
                        next
                    }
                }
            }
        }
        write!(f, ">")
    }
}

impl From<Vec<u32>> for VectorClock {
    fn from(v: Vec<u32>) -> Self {
        VectorClock(v)
    }
}

impl Hash for VectorClock {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let cutoff = self
            .0
            .iter()
            .rposition(|elem| elem != &0)
            .map(|i| i + 1)
            .unwrap_or(0);
        self.0[..cutoff].hash(state);
    }
}

impl PartialEq for VectorClock {
    fn eq(&self, rhs: &Self) -> bool {
        for i in 0..max(self.0.len(), rhs.0.len()) {
            let lhs_elem = self.0.get(i).unwrap_or(&0);
            let rhs_elem = rhs.0.get(i).unwrap_or(&0);
            if lhs_elem != rhs_elem {
                return false;
            }
        }
        true
    }
}

impl PartialOrd for VectorClock {
    fn partial_cmp(&self, rhs: &Self) -> Option<Ordering> {
        let mut expected_ordering = Ordering::Equal;
        for i in 0..max(self.0.len(), rhs.0.len()) {
            let ordering = {
                let lhs_elem = self.0.get(i).unwrap_or(&0);
                let rhs_elem = rhs.0.get(i).unwrap_or(&0);
                lhs_elem.cmp(rhs_elem)
            };

            // The algorithm starts by expecting the vectors to be equal. Once it finds a
            // `Less`/`Greater` element, it expects all other elements to be that or `Equal`
            // (e.g. `[1, *2*, ...]` vs `[1, *3*, ...]` would switch from `Equal` to `Less`).
            // If the ordering switches again then the vectors are incomparable (e.g. continuing
            // the earlier example `[1, 2, *4*]` vs `[1, 3, 0]` would return `None`).
            if expected_ordering == Ordering::Equal {
                expected_ordering = ordering;
            } else if ordering != expected_ordering && ordering != Ordering::Equal {
                return None;
            }
        }
        Some(expected_ordering)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn can_display() {
        assert_eq!(format!("{}", vclock![1, 2, 3, 4]), "<1 2 3 4>");

        // Notably equal vectors don't necessarily display the same.
        assert_eq!(format!("{}", vclock![]), "<>");
        assert_eq!(format!("{}", vclock![0]), "<0>");
    }

    #[test]
    fn can_equate() {
        assert_eq!(vclock![], vclock![]);
        assert_eq!(vclock![0], vclock![]);
        assert_eq!(vclock![], vclock![0]);

        assert_ne!(vclock![], vclock![1]);
        assert_ne!(vclock![1], vclock![]);
    }

    #[test]
    fn can_hash() {
        use std::collections::hash_map::DefaultHasher;

        macro_rules! assert_hash_eq {
            ($v1:expr, $v2:expr) => {
                let mut h1 = DefaultHasher::new();
                let mut h2 = DefaultHasher::new();
                $v1.hash(&mut h1);
                $v2.hash(&mut h2);
                assert_eq!(h1.finish(), h2.finish());
            };
        }
        macro_rules! assert_hash_ne {
            ($v1:expr, $v2:expr) => {
                let mut h1 = DefaultHasher::new();
                let mut h2 = DefaultHasher::new();
                $v1.hash(&mut h1);
                $v2.hash(&mut h2);
                assert_ne!(h1.finish(), h2.finish());
            };
        }

        // same hash if equal
        assert_hash_eq!(vclock![], vclock![]);
        assert_hash_eq!(vclock![], vclock![0, 0]);
        assert_hash_eq!(vclock![1], vclock![1, 0]);

        // otherwise hash varies w/ high probability
        assert_hash_ne!(vclock![], vclock![1]);
        assert_hash_ne!(vclock![1], vclock![]);
    }

    #[test]
    fn can_increment() {
        let mut x = vclock![];
        x.increment(2);
        assert_eq!(x, vclock![0, 0, 1]);

        let mut y = vclock![];
        y.increment(2);
        y.increment(0);
        y.increment(2);
        assert_eq!(y, vclock![1, 0, 2]);
    }

    #[test]
    fn can_merge() {
        let mut x = vclock![1, 2, 3, 4];
        x.merge_in(&vclock![5, 6, 0]);
        assert_eq!(x, vclock![5, 6, 3, 4]);

        let mut y = vclock![1, 0, 2];
        y.merge_in(&vclock![3, 1, 0, 4]);
        assert_eq!(y, vclock![3, 1, 2, 4]);
    }

    #[test]
    fn can_order_partially() {
        use Ordering::*;

        // Clocks with matching elements are equal. Missing elements are implicitly zero.
        assert_eq!(Some(Equal), vclock![].partial_cmp(&vclock![]));
        assert_eq!(Some(Equal), vclock![].partial_cmp(&vclock![0, 0]));
        assert_eq!(Some(Equal), vclock![0, 0].partial_cmp(&vclock![]));
        assert_eq!(Some(Equal), vclock![1, 2, 0].partial_cmp(&vclock![1, 2]));

        // A clock is less if at least one element is less and the rest are
        // less-than-or-equal.
        assert_eq!(Some(Less), vclock![].partial_cmp(&vclock![1]));
        assert_eq!(Some(Less), vclock![1, 2, 3].partial_cmp(&vclock![1, 3, 4]));
        assert_eq!(Some(Less), vclock![1, 2, 3].partial_cmp(&vclock![1, 3, 3]));
        assert_eq!(Some(Less), vclock![1, 2, 3].partial_cmp(&vclock![2, 3, 3]));

        // A clock is greater if at least one element is greater and the rest are
        // greater-than-or-equal.
        assert_eq!(Some(Greater), vclock![1].partial_cmp(&vclock![]));
        assert_eq!(
            Some(Greater),
            vclock![1, 2, 3].partial_cmp(&vclock![1, 1, 2])
        );
        assert_eq!(
            Some(Greater),
            vclock![1, 2, 3].partial_cmp(&vclock![1, 1, 3])
        );
        assert_eq!(
            Some(Greater),
            vclock![1, 2, 4].partial_cmp(&vclock![0, 1, 3])
        );

        // If one element is greater while another is less, then the vectors are incomparable.
        assert_eq!(None, vclock![1, 2, 3].partial_cmp(&vclock![1, 3, 2]));
        assert_eq!(None, vclock![1, 2, 3].partial_cmp(&vclock![3, 2, 1]));
        assert_eq!(None, vclock![1, 2, 2].partial_cmp(&vclock![2, 1, 2]));
    }
}
