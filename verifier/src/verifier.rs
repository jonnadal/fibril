use {
    crate::{trace_tree::TraceTree, RunResult, TraceRecord, Verifier, VerifierConfig, Visitor},
    colorful::{Colorful, HSL},
    fibril_core::{Command, Event, Id, Step},
    std::{
        collections::VecDeque,
        fmt::Debug,
        panic::{catch_unwind, AssertUnwindSafe},
    },
    vector_clock::VectorClock,
};

pub(crate) struct Actor<M> {
    behavior: Box<dyn Step<M>>,
    clock: VectorClock,
    id: Id,
    inbox_by_src: Vec<VecDeque<(M, VectorClock)>>,
    #[allow(clippy::type_complexity)]
    next_event: fn(&mut Actor<M>) -> Option<(VectorClock, Event<M>)>,
    trace_tree: TraceTree<M>,
}

impl<M> VerifierConfig<M> {
    fn new(cfg_fn: &impl Fn(&mut VerifierConfig<M>)) -> Self {
        let mut cfg = VerifierConfig {
            behaviors: Vec::new(),
        };
        cfg_fn(&mut cfg);
        cfg
    }

    pub fn spawn(&mut self, behavior: impl Step<M> + 'static) -> Id {
        let id = self.behaviors.len().into();
        self.behaviors.push(Box::new(behavior));
        id
    }
}

impl<M> Verifier<M>
where
    M: Clone + Debug + PartialEq,
{
    fn find_next_reversible_race(&mut self) -> Option<Vec<(Id, Event<M>, VectorClock)>>
    where
        M: Clone + PartialEq,
    {
        for (i, ri) in self.trace_records.iter().enumerate().rev() {
            if !matches!(ri.event, Event::RecvOk(_, _)) {
                continue; // b/c event is deterministic
            }
            for (j, rj) in self.trace_records.iter().enumerate().take(i).rev() {
                if rj.id != ri.id {
                    continue; // b/c event is for a different actor
                }
                if !matches!(rj.event, Event::RecvOk(_, _)) {
                    continue; // b/c event is deterministic
                }
                if ri.event_clock.partial_cmp(&rj.clock) == Some(std::cmp::Ordering::Greater) {
                    continue; // b/c not enabled earlier
                }
                if self.actors[rj.id].trace_tree.visited(
                    self.trace_records
                        .iter()
                        .take(j)
                        .filter(|r| r.id == rj.id)
                        .map(|r| &r.event),
                    &ri.event,
                ) {
                    continue; // b/c already visited
                }

                let mut race: Vec<_> = self
                    .trace_records
                    .iter()
                    .take(j)
                    .map(|r| (r.id, r.event.clone(), r.event_clock.clone()))
                    .collect();
                for r in self.trace_records.iter().skip(j + 1).take(i - j - 1) {
                    if r.clock <= ri.event_clock {
                        race.push((r.id, r.event.clone(), r.event_clock.clone()));
                    }
                }
                race.push((ri.id, ri.event.clone(), ri.event_clock.clone()));
                return Some(race);
            }
        }
        None
    }

    pub fn new(cfg_fn: impl Fn(&mut VerifierConfig<M>) + 'static) -> Self {
        let cfg = VerifierConfig::new(&cfg_fn);

        let count = cfg.behaviors.len();
        let mut actors = Vec::with_capacity(count);
        for (id, behavior) in cfg.behaviors.into_iter().enumerate() {
            actors.push(Actor {
                behavior,
                clock: VectorClock::new_with_len(count),
                id: id.into(),
                inbox_by_src: vec![VecDeque::new(); count],
                next_event: |actor| Some((VectorClock::new(), Event::SpawnOk(actor.id))),
                trace_tree: TraceTree::new(),
            });
        }
        Verifier {
            actors,
            cfg_fn: Box::new(cfg_fn),
            next_prefix: Vec::new(),
            trace_records: Vec::new(),
            visitors: Vec::new(),
        }
    }

    fn next_step(&mut self) -> Option<(Id, VectorClock, Event<M>)> {
        for id in 0..self.actors.len() {
            let actor = &mut self.actors[id];
            if let Some((event_clock, event)) = (actor.next_event)(actor) {
                return Some((Id::from(id), event_clock, event));
            }
        }
        None
    }

    fn reset_actors(&mut self) {
        let mut cfg = VerifierConfig::new(&self.cfg_fn);
        for (id, behavior) in cfg.behaviors.drain(..).enumerate() {
            let actor = &mut self.actors[id];
            actor.behavior = behavior;
            actor.clock.reset();
            for inbox in &mut actor.inbox_by_src {
                inbox.clear();
            }
            actor.next_event = |actor| Some((VectorClock::new(), Event::SpawnOk(actor.id)));
            actor.trace_tree.reset_cursor();
        }
        self.trace_records.clear();
    }

    pub fn run(&mut self) -> RunResult<M> {
        let mut trace_count = 0;
        while trace_count < 1024 {
            trace_count += 1;
            println!("\n=== Maximal {} ===", trace_count);
            let prefix = self.next_prefix.drain(..).collect();
            if let Err(panic) = catch_unwind(AssertUnwindSafe(|| self.run_until_maximal(prefix))) {
                let message = if let Some(panic) = panic.downcast_ref::<&'static str>() {
                    panic.to_string()
                } else if let Some(panic) = panic.downcast_ref::<String>() {
                    panic.clone()
                } else {
                    "UNKNOWN".to_string()
                };
                self.trace_records.last_mut().unwrap().command = Command::Panic(message.clone());
                for v in &mut self.visitors {
                    v.on_maximal(&self.trace_records);
                }
                let final_clock = &self.trace_records.last().unwrap().clock;
                return RunResult::Panic {
                    message,
                    minimal_trace: self
                        .trace_records
                        .iter()
                        .filter(|r| &r.clock <= final_clock) // minimal trace
                        .cloned()
                        .collect(),
                };
            }
            self.next_prefix = match self.find_next_reversible_race() {
                None => return RunResult::Complete,
                Some(race) => race,
            };
        }
        RunResult::Incomplete
    }

    fn run_until_maximal(&mut self, prefix: Vec<(Id, Event<M>, VectorClock)>) {
        self.reset_actors();

        for (id, event, event_clock) in prefix {
            if let Event::RecvOk(src, _msg) = &event {
                let actor = &mut self.actors[id];
                let inbox = &mut actor.inbox_by_src[*src];
                let (_m, m_clock) = match inbox.pop_front() {
                    None => panic!("- Inbox empty. id={id:?}"),
                    Some(pair) => pair,
                };
                actor.clock.merge_in(&m_clock);
            }
            self.step(id, event, event_clock);
        }

        while let Some((id, event_clock, event)) = self.next_step() {
            self.step(id, event, event_clock);
        }

        for v in &mut self.visitors {
            v.on_maximal(&self.trace_records);
        }
    }

    fn step(&mut self, id: Id, event: Event<M>, event_clock: VectorClock) {
        let actors = &mut self.actors;

        let color = HSL::new(1.0 * usize::from(id) as f32 / actors.len() as f32, 0.5, 0.5);
        let actor = &mut actors[id];
        actor.clock.increment(id.into());
        match &event {
            Event::SpawnOk(id) => println!(
                "{}",
                format!("► {clock} SpawnOk → {id}", clock = actor.clock).color(color)
            ),
            Event::RecvOk(src, m) => println!(
                "{}",
                format!(
                    "► {clock} RecvOk[{src} → {m:?}] → {id}",
                    clock = actor.clock
                )
                .color(color)
            ),
            Event::SendOk => println!(
                "{}",
                format!("► {clock} SendOk → {id}", clock = actor.clock).color(color)
            ),
            _ => unimplemented!(),
        }
        self.trace_records.push(TraceRecord {
            event: event.clone(),
            event_clock,
            id,
            command: Command::Panic("PLACEHOLDER".to_string()),
            clock: actor.clock.clone(),
        });
        let record = self.trace_records.last_mut().unwrap();
        let command = actor.behavior.step(event);
        let clock = &actor.clock;
        actors[id].next_event = match &command {
            Command::Exit => {
                println!("{}", format!("■ {clock} {id} → Exit").color(color));
                |_| None
            }
            Command::Panic(msg) => {
                println!(
                    "{}",
                    format!("■ {clock} {id} → Panic({msg:?})").color(color)
                );
                |_| None
            }
            Command::Recv => {
                println!("{}", format!("■ {clock} {id} → Recv").color(color));
                |actor| {
                    for (src, inbox) in actor.inbox_by_src.iter_mut().enumerate() {
                        let (m, m_clock) = match inbox.pop_front() {
                            None => continue,
                            Some(pair) => pair,
                        };
                        actor.clock.merge_in(&m_clock);
                        return Some((m_clock, Event::RecvOk(src.into(), m)));
                    }
                    None
                }
            }
            Command::Send(dst, m) => {
                println!(
                    "{}",
                    format!("■ {clock} {id} → Send[{m:?} → {dst}]").color(color)
                );
                let m_clock = actor.clock.clone();
                actors[*dst].inbox_by_src[id].push_back((m.clone(), m_clock));
                |_| Some((VectorClock::new(), Event::SendOk))
            }
            _ => unimplemented!(),
        };
        record.command = command;
        actors[id].trace_tree.visit(record.clone());
    }

    pub fn visitor(mut self, visitor: impl Visitor<M> + 'static) -> Self {
        self.visitors.push(Box::new(visitor));
        self
    }
}
