use {
    crate::TraceRecord,
    fibril_core::Event,
    std::{
        cell::RefCell,
        fmt::{self, Debug, Display, Formatter},
        rc::Rc,
    },
};

#[derive(Debug)]
pub(crate) enum TraceTree<M> {
    Empty,
    Full {
        root: Rc<RefCell<TraceTreeNode<M>>>,
        // Remember to use Rc::ptr_eq for checking which node this points to. As a safety
        // precaution, TraceTreeNode does not even support value equality checks.
        cursor: Option<Rc<RefCell<TraceTreeNode<M>>>>,
    },
}

impl<M> TraceTree<M> {
    pub(crate) fn new() -> Self {
        TraceTree::Empty
    }

    pub(crate) fn visit(&mut self, record: TraceRecord<M>)
    where
        M: Clone + PartialEq,
    {
        match self {
            TraceTree::Empty => {
                let root = Rc::new(RefCell::new(TraceTreeNode {
                    record,
                    nexts: Vec::new(),
                }));
                *self = TraceTree::Full {
                    cursor: Some(Rc::clone(&root)),
                    root,
                };
            }
            TraceTree::Full {
                cursor: new_cursor @ None,
                root,
            } => {
                *new_cursor = Some(Rc::clone(root));
            }
            TraceTree::Full { cursor, .. } => {
                let position = cursor
                    .as_ref()
                    .unwrap()
                    .borrow()
                    .nexts
                    .iter()
                    .position(|node| node.borrow().record.event == record.event);
                match position {
                    None => {
                        let next = Rc::new(RefCell::new(TraceTreeNode {
                            record,
                            nexts: Vec::new(),
                        }));
                        let next_cursor = Rc::clone(&next);
                        cursor.as_ref().unwrap().borrow_mut().nexts.push(next);
                        *cursor = Some(next_cursor);
                    }
                    Some(position) => {
                        let x = Rc::clone(&cursor.as_ref().unwrap().borrow().nexts[position]);
                        *cursor = Some(x);
                    }
                }
            }
        }
    }

    pub(crate) fn visited<'a>(
        &self,
        mut prefix: impl Iterator<Item = &'a Event<M>> + Debug,
        last: &Event<M>,
    ) -> bool
    where
        M: PartialEq + 'a,
    {
        let mut cursor = match self {
            TraceTree::Empty => return false,
            TraceTree::Full { root, .. } => {
                let spawn_event = match prefix.next() {
                    None => panic!("Prefix empty. This indicates a bug."),
                    Some(spawn) => spawn,
                };
                assert!(matches!(spawn_event, Event::SpawnOk(_)));
                if &root.borrow().record.event != spawn_event {
                    panic!("Spawn event did not match. This indicates a bug.");
                }
                Rc::clone(root)
            }
        };
        for next in prefix {
            let matching = match cursor
                .borrow()
                .nexts
                .iter()
                .find(|node| &node.borrow().record.event == next)
            {
                None => panic!("Invalid prefix."),
                Some(cursor) => Rc::clone(cursor),
            };
            cursor = matching;
        }
        let visited = cursor
            .borrow()
            .nexts
            .iter()
            .any(|node| &node.borrow().record.event == last);
        visited
    }

    pub(crate) fn reset_cursor(&mut self) {
        if let TraceTree::Full { cursor, .. } = self {
            *cursor = None;
        }
    }
}

impl<M> Display for TraceTree<M>
where
    M: Debug,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), fmt::Error> {
        let (root, cursor) = match self {
            TraceTree::Empty => return write!(f, "TraceTree::Empty"),
            TraceTree::Full { root, cursor } => (root, cursor),
        };
        write!(f, "TraceTree::Full")?;

        let mut stack = vec![(0, Rc::clone(root))];
        while let Some((depth, node)) = stack.pop() {
            let suffix = match cursor {
                Some(other_node) if Rc::ptr_eq(&node, other_node) => " ⇐",
                _ => "",
            };
            write!(
                f,
                "\n\t{:width$}- {rec}{suffix}",
                "",
                width = 2 * depth,
                rec = node.borrow().record
            )?;
            for next in &node.borrow().nexts {
                stack.push((depth + 1, Rc::clone(next)));
            }
        }

        Ok(())
    }
}

// Lack of PartialEq/Eq is intentional to reduce the chance of a value-comparison since
// currently the only use case is pointer comparison against the cursor.
#[derive(Clone, Debug)]
pub(crate) struct TraceTreeNode<M> {
    record: TraceRecord<M>,
    nexts: Vec<Rc<RefCell<TraceTreeNode<M>>>>,
}
