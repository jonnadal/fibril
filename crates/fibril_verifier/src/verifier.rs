use {
    crate::{time_context::TimeContext, trace_tree::TraceTree, TraceRecord, Visitor},
    colorful::Colorful,
    fibril::Fiber,
    fibril_core::{Command, Event, Expectation, Id, Step},
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
    enabled_events: EnabledEventIterator<M>,
    expectation: Option<String>,
    inbox_by_src: Vec<VecDeque<(M, VectorClock)>>,
    time_context: TimeContext,
    trace_tree: TraceTree<M>,
}

/// Iterates over enabled events for a particular [`Actor`].
///
/// # Purpose
///
/// Normally the model checker only needs to determine one enabled event to step next, but when running in
/// interactive mode, the operator may want to choose a different enabled event. This data
/// structure enables that by enumerating all enabled evants.
///
/// # Usage
///
/// There are two entry points:
///
/// 1. [`Verifier::into_fiber`] for interactive mode calls [`Verifier::enabled_steps`], which
///    enumerates [`EnabledEventIterator::next`] for each actor.
/// 2. [`Verifier::run`] for non-interactive mode calls [`Verifier::pick_next_step`], which calls
///    [`EnabledEventIterator::next`] until an actor has a next step.
#[derive(Clone)]
enum EnabledEventIterator<M> {
    Deterministic(Option<Event<M>>),
    DeadlineElapsed { remaining: Vec<bool> },
    Recv { src: Id, inbox_offset: usize },
}

impl<M> EnabledEventIterator<M> {
    fn next(
        &mut self,
        inbox_by_src: &[VecDeque<(M, VectorClock)>],
    ) -> Option<(VectorClock, Event<M>)>
    where
        M: Clone,
    {
        match self {
            EnabledEventIterator::Deterministic(option) => {
                option.take().map(|event| (VectorClock::new(), event))
            }
            EnabledEventIterator::DeadlineElapsed { remaining } => remaining
                .pop()
                .map(|truth| (VectorClock::new(), Event::DeadlineElapsedOk(truth))),
            EnabledEventIterator::Recv { src, inbox_offset } => loop {
                let inbox = match inbox_by_src.get(usize::from(*src)) {
                    None => return None,
                    Some(inbox) => inbox,
                };
                let msg = inbox.get(*inbox_offset);
                match msg {
                    None => {
                        *src = Id::from(usize::from(*src) + 1);
                        *inbox_offset = 0;
                        continue;
                    }
                    Some((msg, msg_clock)) => {
                        *inbox_offset += 1;
                        return Some((msg_clock.clone(), Event::RecvOk(*src, msg.clone())));
                    }
                }
            },
        }
    }
}

#[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[non_exhaustive]
pub enum RunResult<M> {
    Complete,
    Incomplete,
    Panic {
        message: String,
        minimal_trace: Vec<TraceRecord<M>>,
    },
    UnmetExpectation {
        description: String,
        id: Id,
        trace: Vec<TraceRecord<M>>,
    },
}

pub struct BehaviorConfig<M> {
    behaviors: Vec<Box<dyn Step<M>>>,
}

impl<M> BehaviorConfig<M> {
    fn create_actors(cfg_fn: impl Fn(&mut BehaviorConfig<M>)) -> Vec<Actor<M>> {
        let mut cfg = BehaviorConfig {
            behaviors: Vec::new(),
        };
        cfg_fn(&mut cfg);
        let count = cfg.behaviors.len();
        cfg.behaviors
            .into_iter()
            .enumerate()
            // NB: Update reset_actors whenever adding a field here.
            .map(|(idx, behavior)| Actor {
                behavior,
                clock: VectorClock::new_with_len(count),
                enabled_events: EnabledEventIterator::Deterministic(Some(Event::SpawnOk(
                    Id::from(idx),
                ))),
                expectation: None,
                inbox_by_src: {
                    let mut inbox_by_src = Vec::new();
                    for _ in 0..count {
                        inbox_by_src.push(VecDeque::new());
                    }
                    inbox_by_src
                },
                time_context: TimeContext::default(),
                trace_tree: TraceTree::new(),
            })
            .collect()
    }

    fn reset_actors(cfg_fn: impl Fn(&mut BehaviorConfig<M>), actors: &mut [Actor<M>]) {
        let mut cfg = BehaviorConfig {
            behaviors: Vec::new(),
        };
        cfg_fn(&mut cfg);
        for ((id, actor), behavior) in actors.iter_mut().enumerate().zip(cfg.behaviors) {
            actor.behavior = behavior;
            actor.clock.reset();
            actor.enabled_events =
                EnabledEventIterator::Deterministic(Some(Event::SpawnOk(id.into())));
            actor.expectation = None;
            for inbox in &mut actor.inbox_by_src {
                inbox.clear();
            }
            actor.time_context.reset();
            actor.trace_tree.reset_cursor();
        }
    }

    pub fn spawn(&mut self, behavior: impl Step<M> + 'static) -> Id {
        let id = self.behaviors.len().into();
        self.behaviors.push(Box::new(behavior));
        id
    }
}

/// A runtime that can verify safety properties observable by distributed [`Step`] instances.
///
/// # Purpose
///
/// In the presence of nondeterminism, most runtimes execute one "schedule" of events. For
/// instance, if clients _c<sub>A</sub>_ and _c<sub>B</sub>_ send two racing messages `A` and
/// `B` to server _s_, then a typical scheduler will either exercise the case where message `A`
/// lands before message `B` or the case where `B` lands before `A`. This runtime will exercise
/// both.
///
/// By exercising all anticipated nondeterministic outcomes (even seemingly unlikely ones like
/// unbounded message delivery delay), we can infer that any assertions that pass during
/// verification should also hold during real-world scenarios.
///
/// # Internal Implementation Details
///
/// ```text
/// You can safely ignore this section if you only want to use the library. It is only
/// included as documentation for library contributors.
/// ```
///
/// The implementation is based on my understanding of _[Source Sets: A Foundation for Optimal
/// Dynamic Partial Order Reduction](https://dl.acm.org/doi/10.1145/3073408)_ (and feedback is very
/// much appreciated if I got this wrong):
///
/// 1. Each sequential behavior implementing [`Step`] is paired with a _wakeup tree_.  Each tree
///    persists the sequenced events experienced by that actor for all visited system traces.
/// 2. When evaluating whether to schedule a potential [`Event`] for an actor, the scheduler
///    first checks that actor's wakeup tree. It only schedules events not yet in the tree.
/// 3. Conversely, when an actor triggers a [`Command`] that generates an [`Event`], the event is
///    added to a queue for the recipient actor.
/// 4. The checker primes the queue by spawning actors and continues until all queues are
///    empty.
///
/// Two planned optimizations:
///
/// 1. I believe the algorithm only relies on recording events corresponding with nondeterministic
///    commands (such as [`Event::RecvOk`] for [`Command::Recv`]) in the wakeup trees. For
///    debuggability the current implementation also records events corresponding with
///    deterministic commands (such as [`Event::SendOk`] for [`Command::Send`]).
/// 2. The implementation could persist event _fingerprints_ (digests) rather than the specific
///    events to reduce memory consumption at the cost of added CPU load.
pub struct Verifier<M> {
    #[allow(clippy::type_complexity)]
    cfg_fn: Box<dyn Fn(&mut BehaviorConfig<M>)>,
    visitors: Vec<Box<dyn Visitor<M>>>,
}

impl<M> Verifier<M>
where
    M: Clone + Debug + PartialEq,
{
    pub fn new(cfg_fn: impl Fn(&mut BehaviorConfig<M>) + 'static) -> Self {
        Verifier {
            cfg_fn: Box::new(cfg_fn),
            visitors: Vec::new(),
        }
    }

    pub fn visitor(mut self, visitor: impl Visitor<M> + 'static) -> Self {
        self.visitors.push(Box::new(visitor));
        self
    }

    #[track_caller]
    pub fn assert_no_panic(&mut self) {
        match self.run() {
            RunResult::Complete => (),
            RunResult::Incomplete => panic!("Too many representative traces."),
            RunResult::Panic {
                message,
                minimal_trace,
            } => {
                println!("Minimal trace reaching panic:");
                let mut i = 1;
                for r in &minimal_trace {
                    println!("\t{i}. {r}");
                    i += 1;
                }
                panic!("Panic {message:?}");
            }
            RunResult::UnmetExpectation {
                description,
                id,
                trace,
            } => {
                println!("Trace reaching unmet expectation:");
                let mut i = 1;
                for r in &trace {
                    println!("\t{i}. {r}");
                    i += 1;
                }
                panic!("{id} did not meet expectation {description:?}");
            }
        }
    }

    #[track_caller]
    pub fn assert_panic(&mut self) -> (String, Vec<TraceRecord<M>>) {
        match self.run() {
            RunResult::Complete => panic!("Done, but expected an actor to panic."),
            RunResult::Incomplete => panic!("Too many representative traces."),
            RunResult::Panic {
                message,
                minimal_trace,
            } => (message, minimal_trace),
            RunResult::UnmetExpectation {
                description,
                id,
                trace,
            } => {
                println!("Trace reaching unmet expectation:");
                let mut i = 1;
                for r in &trace {
                    println!("\t{i}. {r}");
                    i += 1;
                }
                panic!("{id} did not meet expectation {description:?}");
            }
        }
    }

    #[track_caller]
    pub fn assert_unmet_expectation(&mut self, expected: impl ToString) {
        match self.run() {
            RunResult::Complete => panic!("Done, but expected unmet expectation."),
            RunResult::Incomplete => panic!("Too many representative traces."),
            RunResult::Panic {
                message,
                minimal_trace,
            } => {
                println!("Minimal trace reaching panic:");
                let mut i = 1;
                for r in &minimal_trace {
                    println!("\t{i}. {r}");
                    i += 1;
                }
                panic!("Panic {message:?}");
            }
            RunResult::UnmetExpectation { description, .. } => {
                assert_eq!(description, expected.to_string());
            }
        }
    }

    fn enabled_steps(actors: &[Actor<M>]) -> Vec<(Event<M>, VectorClock, Id)> {
        let mut output = Vec::new();
        for (idx, actor) in actors.iter().enumerate() {
            let mut enabled_events = actor.enabled_events.clone();
            while let Some((event_clock, event)) = enabled_events.next(&actor.inbox_by_src) {
                output.push((event, event_clock, Id::from(idx)));
            }
        }
        output
    }

    fn pick_next_representative_prefix(
        trace_records: &[TraceRecord<M>],
        actors: &[Actor<M>],
    ) -> Option<Vec<(Id, Event<M>, VectorClock)>>
    where
        M: Clone + PartialEq,
    {
        for (i, ri) in trace_records.iter().enumerate().rev() {
            match &ri.event {
                Event::DeadlineElapsedOk(false) => {
                    // The `false` case is always visited before `true`, and in every such case, it
                    // is also possible that the deadline has elapsed.  This eliminates the need to
                    // reference the trace tree to see if a particular ordering has already been
                    // visited.
                    let mut race: Vec<_> = trace_records
                        .iter()
                        .take(i)
                        .map(|r| (r.id, r.event.clone(), r.event_clock.clone()))
                        .collect();
                    race.push((ri.id, Event::DeadlineElapsedOk(true), VectorClock::new()));
                    if std::env::var("FIBRIL_DEBUG").is_ok() {
                        println!("Trace records with pending reversal:");
                        for (k, r) in trace_records.iter().enumerate() {
                            let msg = format!("{k: >3}{}. {r}", if k == i { " (i)" } else { "" });
                            if k == i {
                                println!("{}", msg.color(colorful::Color::Red));
                            } else {
                                println!("{msg}");
                            }
                        }
                    }
                    return Some(race);
                }
                Event::RecvOk(src_i, _msg_i) => {
                    // A `RecvOk` record  has been established at step `i`. Continue walking to
                    // find a `RecvOk` record for the same recipient earlier in the trace (step
                    // `j`).  Return a prefix that reverses the race if the event at step `i` was
                    // enabled earlier at step `j` (i.e. if the event at step `i` is not "caused
                    // by" the completion of step `j`) but was not yet visited in the trace tree.
                    // For instance...
                    //
                    // ```text
                    // 0. trace_records[0]: SpawnOk(:0)@<> → :0@<1>
                    // ...
                    // j. trace_records[j]: RecvOk(src1, m1)@ec1 → dst@(ac1 + ec1)
                    // ...
                    // i. trace_records[i]: RecvOk(src2, m2)@ec2 → dst@(ac2 + ec2)
                    // ...
                    // ```
                    //
                    // ... is reversable if `ec2` is not greater than or equal to `ac1 + ec1`,
                    // becoming the following (and note that the racing delivery might not be
                    // scheduled as it's possible that the recipient exited or panicked after the
                    // reversal):
                    //
                    // ```text
                    // 0. trace_records[0]: SpawnOk(0)@<> → :0@<1>
                    // ...
                    // j. trace_records[i]: RecvOk(src2, m2)@ec2 → dst@(ac1 + ec2)
                    // ...
                    // ```
                    for (j, rj) in trace_records.iter().enumerate().take(i).rev() {
                        if rj.id != ri.id {
                            continue; // b/c event is for a different actor
                        }
                        let (src_j, _msg_j) = if let Event::RecvOk(src, msg) = &rj.event {
                            (src, msg)
                        } else {
                            continue; // b/c event is not a racing recv
                        };
                        if ri.event_clock >= rj.clock {
                            continue; // b/c event is a prerequisite to arrive at i
                        }
                        if src_i == src_j {
                            continue; // b/c queued
                        }
                        // The above condition handles the case where the events for i and j are from
                        // the same sender. We also have to compare the events *between* i and j.
                        if trace_records.iter().take(i).skip(j + 1).any(|r| {
                            if let Event::RecvOk(src_r, _msg_r) = &r.event {
                                src_i == src_r
                            } else {
                                false
                            }
                        }) {
                            continue; // b/c queued
                        }
                        if actors[rj.id].trace_tree.visited(
                            trace_records
                                .iter()
                                .take(j)
                                .filter(|r| r.id == rj.id)
                                .map(|r| &r.event),
                            &ri.event,
                        ) {
                            continue; // b/c already visited
                        }

                        // At this point we've established a reversable race and will return it.
                        let mut race: Vec<_> = trace_records
                            .iter()
                            .take(j)
                            .map(|r| (r.id, r.event.clone(), r.event_clock.clone()))
                            .collect();
                        for r in trace_records.iter().skip(j + 1).take(i - j - 1) {
                            if r.clock <= ri.event_clock {
                                race.push((r.id, r.event.clone(), r.event_clock.clone()));
                            }
                        }
                        race.push((ri.id, ri.event.clone(), ri.event_clock.clone()));
                        if std::env::var("FIBRIL_DEBUG").is_ok() {
                            println!("Trace records with pending reversal:");
                            for (k, r) in trace_records.iter().enumerate() {
                                let msg = format!(
                                    "{k: >3}{}. {r}",
                                    if k == i {
                                        " (i)"
                                    } else if k == j {
                                        " (j)"
                                    } else {
                                        ""
                                    }
                                );
                                if k == i || k == j {
                                    println!("{}", msg.color(colorful::Color::Red));
                                } else {
                                    println!("{msg}");
                                }
                            }
                            println!("Reversal prefix:");
                            for (k, (pid, event, event_clock)) in race.iter().enumerate() {
                                let msg = format!(
                                    "{k: >3}{}. {event:?}@{event_clock} → {pid}",
                                    if k == j { " (j)" } else { "" }
                                );
                                if k >= j {
                                    println!("{}", msg.color(colorful::Color::Red));
                                } else {
                                    println!("{msg}");
                                }
                            }
                        }
                        return Some(race);
                    }
                }
                _ => continue, // b/c event is deterministic
            }
        }
        if std::env::var("FIBRIL_DEBUG").is_ok() {
            println!("Trace records with no pending reversal:");
            for (k, r) in trace_records.iter().enumerate() {
                println!("{k: >3}. {r}");
            }
        }
        None
    }

    fn pick_next_step(actors: &[Actor<M>]) -> Option<(Id, Event<M>, VectorClock)> {
        for (idx, actor) in actors.iter().enumerate() {
            if let Some((event_clock, event)) =
                actor.enabled_events.clone().next(&actor.inbox_by_src)
            {
                return Some((Id::from(idx), event, event_clock));
            }
        }
        None
    }

    pub fn run(&mut self) -> RunResult<M> {
        const MAX_TRACE_COUNT: usize = 1024 * 1024;
        const TRACE_DEBUG_INTERAL: usize = 4 * 1024;

        let mut actors = BehaviorConfig::create_actors(&self.cfg_fn);
        let mut trace_records: Vec<TraceRecord<M>> = Vec::new();

        // This method will explore a maximum number of representative traces before returning.
        let mut trace_count = 0;
        let mut next_prefix: Vec<(Id, Event<M>, VectorClock)> = Vec::new();
        while trace_count < MAX_TRACE_COUNT {
            trace_records.clear();
            BehaviorConfig::reset_actors(&self.cfg_fn, &mut actors);

            trace_count += 1;
            if std::env::var("FIBRIL_DEBUG").is_ok() || trace_count % TRACE_DEBUG_INTERAL == 0 {
                println!("\n=== Maximal {trace_count} ===");
            }

            // First consume the prefix, which is known to not panic.
            for (id, event, event_clock) in next_prefix.drain(..) {
                Self::step(id, event, event_clock, &mut actors, &mut trace_records);
            }

            // Now repeatedly pick and apply a next step until we arrive at a "maximal" trace. The
            // goal of the checker is to enumerate representative traces until a panic is
            // enountered (e.g. due to an assertion violation), so the loop is wrapped in
            // catch_unwind.
            let result = catch_unwind(AssertUnwindSafe(|| loop {
                let (id, event, event_clock) = match Self::pick_next_step(&actors) {
                    None => break,
                    Some(tuple) => tuple,
                };
                Self::step(id, event, event_clock, &mut actors, &mut trace_records);
            }));

            // Handle the panic case by completing the trace and returning a filtered subset of
            // steps (as not all steps are necessarily relevant).
            if let Err(panic) = result {
                let message = if let Some(panic) = panic.downcast_ref::<&'static str>() {
                    panic.to_string()
                } else if let Some(panic) = panic.downcast_ref::<String>() {
                    panic.clone()
                } else {
                    "UNKNOWN".to_string()
                };
                let last_trace_record = trace_records.last_mut().unwrap();
                last_trace_record.command = Command::Panic(message.clone());
                for v in &mut self.visitors {
                    v.on_maximal(&trace_records);
                }
                let final_clock = &trace_records.last().unwrap().clock;
                return RunResult::Panic {
                    message,
                    minimal_trace: trace_records
                        .iter()
                        .filter(|r| &r.clock <= final_clock) // minimal trace
                        .cloned()
                        .collect(),
                };
            }

            // If the code reaches this point, then the trace is maximal, and we can check that no
            // actors have any unmet expectations, which provide a way to assert liveness
            // properties on finite executions. Similar to assertions, these expectations are local
            // to each actor, a requirement for partial order reduction.
            for v in &mut self.visitors {
                v.on_maximal(&trace_records);
            }
            if let Some((idx, description)) = actors
                .iter()
                .enumerate()
                .find_map(|(idx, a)| a.expectation.as_ref().map(|e| (idx, e)))
            {
                return RunResult::UnmetExpectation {
                    description: description.clone(),
                    id: Id::from(idx),
                    trace: trace_records.clone(),
                };
            }

            // To maintain *optimal* dynamic partial order reduction, the next visited trace must
            // avoid revisiting any of the already visited equivalence classes (AKA "Mazurkiewicz
            // traces"). To do so, the method picks a prefix that is guaranteed to be disjoint from
            // the previously visited equivalence classes. It does this by referencing the trace
            // records to find race conditions to explore.
            next_prefix = match Self::pick_next_representative_prefix(&trace_records, &actors) {
                None => return RunResult::Complete,
                Some(race) => race,
            };
        }
        RunResult::Incomplete
    }

    fn step(
        id: Id,
        event: Event<M>,
        event_clock: VectorClock,
        actors: &mut [Actor<M>],
        trace_records: &mut Vec<TraceRecord<M>>,
    ) {
        let actor = &mut actors[id];

        // Update the stepped actor's vector clock.
        actor.clock.increment(id.into());
        actor.clock.merge_in(&event_clock);

        // Update the stepped actor's associated state where applicable.
        if let Event::RecvOk(src, expected_msg) = &event {
            let inbox = &mut actor.inbox_by_src[*src];
            let (m, m_clock) = match inbox.pop_front() {
                None => panic!("- Inbox empty. id={id:?}"),
                Some(pair) => pair,
            };
            assert_eq!(
                expected_msg, &m,
                "Unexpected nondeterminism detected when reversing a race. Are you using a
                       SystemTime or random number generator?"
            );
            assert_eq!(m_clock, event_clock);
        } else if let Event::DeadlineElapsedOk(true) = &event {
            let deadline = trace_records
                .iter()
                .rev()
                .find_map(|r| match r.command {
                    Command::DeadlineElapsed(deadline) if r.id == id => Some(deadline),
                    _ => None,
                })
                .expect("Unable to find expected preceding command");
            actor.time_context.deadline_elapsed(deadline);
        }

        // Track potential history in case there's a panic.
        trace_records.push(TraceRecord {
            event: event.clone(),
            event_clock,
            id,
            command: Command::Panic("PLACEHOLDER".to_string()),
            clock: actor.clock.clone(),
        });
        let record = trace_records.last_mut().unwrap();

        // Pass the event to the actor so that it can respond with a command. The command will
        // dictate what events are now applicable for this actor (as these events represent return
        // values, which may be nondeterministic).
        let command = actor.behavior.step(event);
        actors[id].enabled_events = match &command {
            Command::Deadline(duration) => EnabledEventIterator::Deterministic(Some(
                Event::DeadlineOk(actor.time_context.new_deadline(*duration)),
            )),
            Command::DeadlineElapsed(deadline) => EnabledEventIterator::DeadlineElapsed {
                remaining: actor
                    .time_context
                    .possibilities_for_deadline_elapsed(*deadline),
            },
            Command::Exit => EnabledEventIterator::Deterministic(None),
            Command::Expect(description) => {
                if actor.expectation.is_some() {
                    panic!("Expectation already exists: {id} / {description}");
                }
                actor.expectation = Some(description.clone());
                EnabledEventIterator::Deterministic(Some(Event::ExpectOk(Expectation::new(
                    description.clone(),
                ))))
            }
            Command::ExpectationMet(expectation) => {
                if actor.expectation.is_none() {
                    panic!("Expectation does not exist: {id} / {expectation:?}");
                }
                actor.expectation = None;
                EnabledEventIterator::Deterministic(Some(Event::ExpectationMetOk))
            }
            Command::Panic(_) => EnabledEventIterator::Deterministic(None),
            Command::Recv => EnabledEventIterator::Recv {
                src: 0.into(),
                inbox_offset: 0,
            },
            Command::Send(dst, m) => {
                let m_clock = actor.clock.clone();
                actors[*dst].inbox_by_src[id].push_back((m.clone(), m_clock));
                EnabledEventIterator::Deterministic(Some(Event::SendOk))
            }
            Command::SleepUntil(_) => {
                EnabledEventIterator::Deterministic(Some(Event::SleepUntilOk))
            }
            _ => unimplemented!("Command not supported: {command:?}"),
        };

        // Correct the trace record's command, and remember this step in the trace tree so that
        // DPOR can avoid redundant traces.
        record.command = command;
        actors[id].trace_tree.visit(record.clone());
    }

    /// Provides an interactive interface to this `Verifier`.
    pub fn into_fiber(self) -> Fiber<'static, VerifierMsg<M>> {
        let mut actors = BehaviorConfig::create_actors(&self.cfg_fn);
        let mut trace_records = Vec::new();
        Fiber::new(move |sdk| loop {
            let (src, msg) = sdk.recv();
            match msg {
                VerifierMsg::Enabled => {
                    sdk.send(
                        src,
                        VerifierMsg::EnabledOk(
                            Self::enabled_steps(&actors)
                                .into_iter()
                                .map(|(event, event_clock, id)| {
                                    VerifierMsg::Step(event, event_clock, id)
                                })
                                .collect(),
                        ),
                    );
                }
                VerifierMsg::Help => {
                    sdk.send(
                        src,
                        VerifierMsg::HelpOk {
                            start_with: Box::new(VerifierMsg::Enabled),
                        },
                    );
                }
                VerifierMsg::Reset => {
                    trace_records.clear();
                    BehaviorConfig::reset_actors(&self.cfg_fn, &mut actors);
                    sdk.send(
                        src,
                        VerifierMsg::ResetOk {
                            enabled: Self::enabled_steps(&actors)
                                .into_iter()
                                .map(|(event, event_clock, id)| {
                                    VerifierMsg::Step(event, event_clock, id)
                                })
                                .collect(),
                        },
                    );
                }
                VerifierMsg::ResetAndStepMany(steps) => {
                    trace_records.clear();
                    BehaviorConfig::reset_actors(&self.cfg_fn, &mut actors);
                    for (event, event_clock, id) in steps {
                        Self::step(id, event, event_clock, &mut actors, &mut trace_records);
                    }
                    sdk.send(
                        src,
                        VerifierMsg::ResetAndStepManyOk {
                            trace_records: trace_records.clone(),
                            enabled: Self::enabled_steps(&actors)
                                .into_iter()
                                .map(|(event, event_clock, id)| {
                                    VerifierMsg::Step(event, event_clock, id)
                                })
                                .collect(),
                        },
                    );
                }
                VerifierMsg::Step(event, event_clock, id) => {
                    Self::step(id, event, event_clock, &mut actors, &mut trace_records);
                    sdk.send(
                        src,
                        VerifierMsg::StepOk {
                            trace_records: trace_records.clone(),
                            enabled: Self::enabled_steps(&actors)
                                .into_iter()
                                .map(|(event, event_clock, id)| {
                                    VerifierMsg::Step(event, event_clock, id)
                                })
                                .collect(),
                            location: Box::new(VerifierMsg::ResetAndStepMany(
                                trace_records
                                    .iter()
                                    .map(|r| (r.event.clone(), r.event_clock.clone(), r.id))
                                    .collect(),
                            )),
                        },
                    );
                }
                _ => continue,
            }
        })
    }

    // FIXME: assumes identifier is `traces`. Swap w/ a macro.
    pub fn panic_with_trace_snapshots(traces: &Vec<Vec<TraceRecord<M>>>) {
        println!("assert_eq!(traces.len(), {});", traces.len());
        for (trace_idx, trace) in traces.iter().enumerate() {
            println!("assert_trace!(");
            println!("    traces[{}],", trace_idx);
            for r in trace {
                println!("    \"{}\",", format!("{}", r).escape_debug());
            }
            println!(");");
        }
        panic!("^");
    }

    /// Runs the verifier recording traces. Convenience function for tests.
    pub fn traces(cfg_fn: impl Fn(&mut BehaviorConfig<M>) + 'static) -> Vec<Vec<TraceRecord<M>>>
    where
        M: 'static,
    {
        let (record, replay) = crate::TraceRecordingVisitor::new_with_replay();
        let mut verifier = Verifier::new(cfg_fn).visitor(record);
        verifier.run();
        replay()
    }
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum VerifierMsg<M> {
    Enabled,
    EnabledOk(Vec<VerifierMsg<M>>),

    Help,
    HelpOk {
        start_with: Box<VerifierMsg<M>>,
    },

    Reset,
    ResetOk {
        enabled: Vec<VerifierMsg<M>>,
    },

    ResetAndStepMany(Vec<(Event<M>, VectorClock, Id)>),
    ResetAndStepManyOk {
        trace_records: Vec<TraceRecord<M>>,
        enabled: Vec<VerifierMsg<M>>,
    },

    Step(Event<M>, VectorClock, Id),
    StepOk {
        trace_records: Vec<TraceRecord<M>>,
        enabled: Vec<VerifierMsg<M>>,
        location: Box<VerifierMsg<M>>,
    },
}

#[cfg(test)]
mod test {
    use {
        super::*,
        crate::{assert_trace, TraceRecordingVisitor},
        fibril::Fiber,
        std::{
            panic::{catch_unwind, AssertUnwindSafe},
            time::Duration,
        },
    };

    #[test]
    /// Regression test for an earlier bug in the source sets implementation.
    fn does_not_reverse_dependent_recv_ok() {
        let (record, replay) = TraceRecordingVisitor::new_with_replay();
        let mut verifier = Verifier::new(|cfg| {
            let server = cfg.spawn(Fiber::new(|sdk| {
                sdk.recv();
                sdk.send(sdk.id(), "FROM SERVER");
                sdk.recv();
            }));

            cfg.spawn(Fiber::new(move |sdk| {
                sdk.send(server, "FROM CLIENT");
            }));
        })
        .visitor(record);
        verifier.assert_no_panic();
        let traces = replay();
        assert_eq!(traces.len(), 1);
        assert_trace![
            traces[0],
            "SpawnOk(:0)@<> → :0 → Recv@<1 0>",
            "SpawnOk(:1)@<> → :1 → Send(:0, \"FROM CLIENT\")@<0 1>",
            "RecvOk(:1, \"FROM CLIENT\")@<0 1> → :0 → Send(:0, \"FROM SERVER\")@<2 1>",
            "SendOk@<> → :0 → Recv@<3 1>",
            "RecvOk(:0, \"FROM SERVER\")@<2 1> → :0 → Exit@<4 1>",
            "SendOk@<> → :1 → Exit@<0 2>",
        ];
    }

    #[test]
    fn highlights_unmet_expectations() {
        let (record, replay) = TraceRecordingVisitor::new_with_replay();
        let mut verifier = Verifier::new(|cfg| {
            let server = cfg.spawn(Fiber::new(|sdk| loop {
                let (_src, _msg) = sdk.recv();
                // no reply
            }));
            cfg.spawn(Fiber::new(move |sdk| {
                sdk.send(server, "Hello");
                let expect_response = sdk.expect("receive response");
                let _ = sdk.recv();
                sdk.expectation_met(expect_response);
            }));
        })
        .visitor(record);
        if let Err(panic) = catch_unwind(AssertUnwindSafe(|| verifier.assert_no_panic())) {
            if let Some(panic) = panic.downcast_ref::<String>() {
                assert_eq!(panic, ":1 did not meet expectation \"receive response\"");
            } else {
                unreachable!();
            };
        } else {
            panic!("Expected unmet expectation");
        }

        let traces = replay();
        assert_eq!(traces.len(), 1);
    }

    #[test]
    fn ignores_met_expectations() {
        let (record, replay) = TraceRecordingVisitor::new_with_replay();
        let mut verifier = Verifier::new(|cfg| {
            let server = cfg.spawn(Fiber::new(|sdk| loop {
                let (src, msg) = sdk.recv();
                sdk.send(src, msg);
            }));
            cfg.spawn(Fiber::new(move |sdk| {
                sdk.send(server, "Hello");
                let expect_response = sdk.expect("receive response");
                let _ = sdk.recv();
                sdk.expectation_met(expect_response);
            }));
        })
        .visitor(record);
        verifier.assert_no_panic();

        let traces = replay();
        assert_eq!(traces.len(), 1);
    }

    #[test]
    /// When implementing the fix for `does_not_reverse_dependent_recv_ok`, I temporarily broke the
    /// `registry` example. I'm maintaining a minimal repro in this module even though there's
    /// already an integration test for the crate.
    fn reverses_recv_ok_enabled_before_earlier_racing_step_clock() {
        let (record, replay) = TraceRecordingVisitor::new_with_replay();
        let mut verifier = Verifier::new(|cfg| {
            let registry = cfg.spawn(Fiber::new(|sdk| {
                sdk.recv();
                sdk.recv();
                sdk.recv();
            }));
            let worker1 = cfg.spawn(Fiber::new(move |sdk| {
                sdk.recv();
                sdk.send(registry, "W1");
            }));
            let worker2 = cfg.spawn(Fiber::new(move |sdk| {
                sdk.recv();
                sdk.send(registry, "W2");
            }));
            cfg.spawn(Fiber::new(move |sdk| {
                sdk.send(registry, "CLIENT");
                sdk.send(worker1, "GO");
                sdk.send(worker2, "GO");
            }));
        })
        .visitor(record);
        verifier.assert_no_panic();

        let traces = replay();
        assert_eq!(traces.len(), 6); // 3! b/c 3 messages race

        // Case 1: registration order is CLIENT, W1, W2.
        assert_trace![
            traces[0],
            "SpawnOk(:0)@<> → :0 → Recv@<1 0 0 0>",
            "SpawnOk(:1)@<> → :1 → Recv@<0 1 0 0>",
            "SpawnOk(:2)@<> → :2 → Recv@<0 0 1 0>",
            "SpawnOk(:3)@<> → :3 → Send(:0, \"CLIENT\")@<0 0 0 1>",
            "RecvOk(:3, \"CLIENT\")@<0 0 0 1> → :0 → Recv@<2 0 0 1>",
            "SendOk@<> → :3 → Send(:1, \"GO\")@<0 0 0 2>",
            "RecvOk(:3, \"GO\")@<0 0 0 2> → :1 → Send(:0, \"W1\")@<0 2 0 2>",
            "RecvOk(:1, \"W1\")@<0 2 0 2> → :0 → Recv@<3 2 0 2>",
            "SendOk@<> → :1 → Exit@<0 3 0 2>",
            "SendOk@<> → :3 → Send(:2, \"GO\")@<0 0 0 3>",
            "RecvOk(:3, \"GO\")@<0 0 0 3> → :2 → Send(:0, \"W2\")@<0 0 2 3>",
            "RecvOk(:2, \"W2\")@<0 0 2 3> → :0 → Exit@<4 2 2 3>",
            "SendOk@<> → :2 → Exit@<0 0 3 3>",
            "SendOk@<> → :3 → Exit@<0 0 0 4>",
        ];
        // Case 2: registration order is CLIENT, W2, W1.
        assert_trace![
            traces[1],
            "SpawnOk(:0)@<> → :0 → Recv@<1 0 0 0>",
            "SpawnOk(:1)@<> → :1 → Recv@<0 1 0 0>",
            "SpawnOk(:2)@<> → :2 → Recv@<0 0 1 0>",
            "SpawnOk(:3)@<> → :3 → Send(:0, \"CLIENT\")@<0 0 0 1>",
            "RecvOk(:3, \"CLIENT\")@<0 0 0 1> → :0 → Recv@<2 0 0 1>",
            "SendOk@<> → :3 → Send(:1, \"GO\")@<0 0 0 2>",
            "RecvOk(:3, \"GO\")@<0 0 0 2> → :1 → Send(:0, \"W1\")@<0 2 0 2>",
            "SendOk@<> → :3 → Send(:2, \"GO\")@<0 0 0 3>",
            "RecvOk(:3, \"GO\")@<0 0 0 3> → :2 → Send(:0, \"W2\")@<0 0 2 3>",
            "RecvOk(:2, \"W2\")@<0 0 2 3> → :0 → Recv@<3 0 2 3>",
            "RecvOk(:1, \"W1\")@<0 2 0 2> → :0 → Exit@<4 2 2 3>",
            "SendOk@<> → :1 → Exit@<0 3 0 2>",
            "SendOk@<> → :2 → Exit@<0 0 3 3>",
            "SendOk@<> → :3 → Exit@<0 0 0 4>",
        ];
        // Case 3: registration order is W1, CLIENT, W2.
        assert_trace![
            traces[2],
            "SpawnOk(:0)@<> → :0 → Recv@<1 0 0 0>",
            "SpawnOk(:1)@<> → :1 → Recv@<0 1 0 0>",
            "SpawnOk(:2)@<> → :2 → Recv@<0 0 1 0>",
            "SpawnOk(:3)@<> → :3 → Send(:0, \"CLIENT\")@<0 0 0 1>",
            "SendOk@<> → :3 → Send(:1, \"GO\")@<0 0 0 2>",
            "RecvOk(:3, \"GO\")@<0 0 0 2> → :1 → Send(:0, \"W1\")@<0 2 0 2>",
            "RecvOk(:1, \"W1\")@<0 2 0 2> → :0 → Recv@<2 2 0 2>",
            "RecvOk(:3, \"CLIENT\")@<0 0 0 1> → :0 → Recv@<3 2 0 2>",
            "SendOk@<> → :1 → Exit@<0 3 0 2>",
            "SendOk@<> → :3 → Send(:2, \"GO\")@<0 0 0 3>",
            "RecvOk(:3, \"GO\")@<0 0 0 3> → :2 → Send(:0, \"W2\")@<0 0 2 3>",
            "RecvOk(:2, \"W2\")@<0 0 2 3> → :0 → Exit@<4 2 2 3>",
            "SendOk@<> → :2 → Exit@<0 0 3 3>",
            "SendOk@<> → :3 → Exit@<0 0 0 4>",
        ];
        // Case 4: registration order is W1, W2, CLIENT.
        assert_trace![
            traces[3],
            "SpawnOk(:0)@<> → :0 → Recv@<1 0 0 0>",
            "SpawnOk(:1)@<> → :1 → Recv@<0 1 0 0>",
            "SpawnOk(:2)@<> → :2 → Recv@<0 0 1 0>",
            "SpawnOk(:3)@<> → :3 → Send(:0, \"CLIENT\")@<0 0 0 1>",
            "SendOk@<> → :3 → Send(:1, \"GO\")@<0 0 0 2>",
            "RecvOk(:3, \"GO\")@<0 0 0 2> → :1 → Send(:0, \"W1\")@<0 2 0 2>",
            "RecvOk(:1, \"W1\")@<0 2 0 2> → :0 → Recv@<2 2 0 2>",
            "SendOk@<> → :3 → Send(:2, \"GO\")@<0 0 0 3>",
            "RecvOk(:3, \"GO\")@<0 0 0 3> → :2 → Send(:0, \"W2\")@<0 0 2 3>",
            "RecvOk(:2, \"W2\")@<0 0 2 3> → :0 → Recv@<3 2 2 3>",
            "RecvOk(:3, \"CLIENT\")@<0 0 0 1> → :0 → Exit@<4 2 2 3>",
            "SendOk@<> → :1 → Exit@<0 3 0 2>",
            "SendOk@<> → :2 → Exit@<0 0 3 3>",
            "SendOk@<> → :3 → Exit@<0 0 0 4>",
        ];
        // Case 5: registration order is W2, W1, CLIENT.
        assert_trace![
            traces[4],
            "SpawnOk(:0)@<> → :0 → Recv@<1 0 0 0>",
            "SpawnOk(:1)@<> → :1 → Recv@<0 1 0 0>",
            "SpawnOk(:2)@<> → :2 → Recv@<0 0 1 0>",
            "SpawnOk(:3)@<> → :3 → Send(:0, \"CLIENT\")@<0 0 0 1>",
            "SendOk@<> → :3 → Send(:1, \"GO\")@<0 0 0 2>",
            "RecvOk(:3, \"GO\")@<0 0 0 2> → :1 → Send(:0, \"W1\")@<0 2 0 2>",
            "SendOk@<> → :3 → Send(:2, \"GO\")@<0 0 0 3>",
            "RecvOk(:3, \"GO\")@<0 0 0 3> → :2 → Send(:0, \"W2\")@<0 0 2 3>",
            "RecvOk(:2, \"W2\")@<0 0 2 3> → :0 → Recv@<2 0 2 3>",
            "RecvOk(:1, \"W1\")@<0 2 0 2> → :0 → Recv@<3 2 2 3>",
            "RecvOk(:3, \"CLIENT\")@<0 0 0 1> → :0 → Exit@<4 2 2 3>",
            "SendOk@<> → :1 → Exit@<0 3 0 2>",
            "SendOk@<> → :2 → Exit@<0 0 3 3>",
            "SendOk@<> → :3 → Exit@<0 0 0 4>",
        ];
        // Case 5: registration order is W2, CLIENT, W1.
        //
        // This was missed in an earlier version of the fix because <0 0 0 1> ⪯ <3 2 2 3>, and the
        // faulty "fix" was checking for _causally related_ as a condition for _not reversable_.
        assert_trace![
            traces[5],
            "SpawnOk(:0)@<> → :0 → Recv@<1 0 0 0>",
            "SpawnOk(:1)@<> → :1 → Recv@<0 1 0 0>",
            "SpawnOk(:2)@<> → :2 → Recv@<0 0 1 0>",
            "SpawnOk(:3)@<> → :3 → Send(:0, \"CLIENT\")@<0 0 0 1>",
            "SendOk@<> → :3 → Send(:1, \"GO\")@<0 0 0 2>",
            "RecvOk(:3, \"GO\")@<0 0 0 2> → :1 → Send(:0, \"W1\")@<0 2 0 2>",
            "SendOk@<> → :3 → Send(:2, \"GO\")@<0 0 0 3>",
            "RecvOk(:3, \"GO\")@<0 0 0 3> → :2 → Send(:0, \"W2\")@<0 0 2 3>",
            "RecvOk(:2, \"W2\")@<0 0 2 3> → :0 → Recv@<2 0 2 3>",
            "RecvOk(:3, \"CLIENT\")@<0 0 0 1> → :0 → Recv@<3 0 2 3>",
            "RecvOk(:1, \"W1\")@<0 2 0 2> → :0 → Exit@<4 2 2 3>",
            "SendOk@<> → :1 → Exit@<0 3 0 2>",
            "SendOk@<> → :2 → Exit@<0 0 3 3>",
            "SendOk@<> → :3 → Exit@<0 0 0 4>",
        ];
    }

    #[test]
    /// This is a regression test for a bug discovered while implementing the "ABD"
    /// atomic/linearizable register algorithm.
    fn respects_network_queuing_from_self_send() {
        let (record, replay) = TraceRecordingVisitor::new_with_replay();
        let mut verifier = Verifier::new(|cfg| {
            cfg.spawn(Fiber::new(|sdk| {
                sdk.send(sdk.id(), "FIRST IN LINE");
                sdk.send(sdk.id(), "SECOND IN LINE");
                sdk.recv();
                sdk.recv();
            }));
        })
        .visitor(record);
        verifier.assert_no_panic();

        let traces = replay();
        assert_eq!(traces.len(), 1); // cannot reverse the message schedule
        assert_trace![
            traces[0],
            "SpawnOk(:0)@<> → :0 → Send(:0, \"FIRST IN LINE\")@<1>",
            "SendOk@<> → :0 → Send(:0, \"SECOND IN LINE\")@<2>",
            "SendOk@<> → :0 → Recv@<3>",
            "RecvOk(:0, \"FIRST IN LINE\")@<1> → :0 → Recv@<4>",
            "RecvOk(:0, \"SECOND IN LINE\")@<2> → :0 → Exit@<5>",
        ];
    }

    #[test]
    /// This is a regression test for a bug discovered while implementing the "ABD"
    /// atomic/linearizable register algorithm.
    fn respects_network_queuing_from_client_send() {
        let (record, replay) = TraceRecordingVisitor::new_with_replay();
        let mut verifier = Verifier::new(|cfg| {
            let server = cfg.spawn(Fiber::new(|sdk| {
                sdk.recv();
                sdk.recv();
                sdk.recv();
            }));
            cfg.spawn(Fiber::new(move |sdk| {
                sdk.send(server, "FIRST IN LINE FROM C1");
                sdk.send(server, "SECOND IN LINE FROM C1");
            }));
            cfg.spawn(Fiber::new(move |sdk| {
                sdk.send(server, "CONCURRENT FROM C2");
            }));
        })
        .visitor(record);
        verifier.assert_no_panic();

        let traces = replay();
        assert_eq!(traces.len(), 3);

        // Case 1: FIRST IN LINE FROM C1, SECOND IN LINE FROM C1, CONCURRENT FROM C2
        assert_trace![
            traces[0],
            "SpawnOk(:0)@<> → :0 → Recv@<1 0 0>",
            "SpawnOk(:1)@<> → :1 → Send(:0, \"FIRST IN LINE FROM C1\")@<0 1 0>",
            "RecvOk(:1, \"FIRST IN LINE FROM C1\")@<0 1 0> → :0 → Recv@<2 1 0>",
            "SendOk@<> → :1 → Send(:0, \"SECOND IN LINE FROM C1\")@<0 2 0>",
            "RecvOk(:1, \"SECOND IN LINE FROM C1\")@<0 2 0> → :0 → Recv@<3 2 0>",
            "SendOk@<> → :1 → Exit@<0 3 0>",
            "SpawnOk(:2)@<> → :2 → Send(:0, \"CONCURRENT FROM C2\")@<0 0 1>",
            "RecvOk(:2, \"CONCURRENT FROM C2\")@<0 0 1> → :0 → Exit@<4 2 1>",
            "SendOk@<> → :2 → Exit@<0 0 2>",
        ];
        // Case 2: FIRST IN LINE FROM C1, CONCURRENT FROM C2, SECOND IN LINE FROM C1
        assert_trace![
            traces[1],
            "SpawnOk(:0)@<> → :0 → Recv@<1 0 0>",
            "SpawnOk(:1)@<> → :1 → Send(:0, \"FIRST IN LINE FROM C1\")@<0 1 0>",
            "RecvOk(:1, \"FIRST IN LINE FROM C1\")@<0 1 0> → :0 → Recv@<2 1 0>",
            "SendOk@<> → :1 → Send(:0, \"SECOND IN LINE FROM C1\")@<0 2 0>",
            "SpawnOk(:2)@<> → :2 → Send(:0, \"CONCURRENT FROM C2\")@<0 0 1>",
            "RecvOk(:2, \"CONCURRENT FROM C2\")@<0 0 1> → :0 → Recv@<3 1 1>",
            "RecvOk(:1, \"SECOND IN LINE FROM C1\")@<0 2 0> → :0 → Exit@<4 2 1>",
            "SendOk@<> → :1 → Exit@<0 3 0>",
            "SendOk@<> → :2 → Exit@<0 0 2>",
        ];
        // Case 3: CONCURRENT FROM C2, FIRST IN LINE FROM C1, SECOND IN LINE FROM C1
        assert_trace![
            traces[2],
            "SpawnOk(:0)@<> → :0 → Recv@<1 0 0>",
            "SpawnOk(:1)@<> → :1 → Send(:0, \"FIRST IN LINE FROM C1\")@<0 1 0>",
            "SpawnOk(:2)@<> → :2 → Send(:0, \"CONCURRENT FROM C2\")@<0 0 1>",
            "RecvOk(:2, \"CONCURRENT FROM C2\")@<0 0 1> → :0 → Recv@<2 0 1>",
            "RecvOk(:1, \"FIRST IN LINE FROM C1\")@<0 1 0> → :0 → Recv@<3 1 1>",
            "SendOk@<> → :1 → Send(:0, \"SECOND IN LINE FROM C1\")@<0 2 0>",
            "RecvOk(:1, \"SECOND IN LINE FROM C1\")@<0 2 0> → :0 → Exit@<4 2 1>",
            "SendOk@<> → :1 → Exit@<0 3 0>",
            "SendOk@<> → :2 → Exit@<0 0 2>",
        ];
    }

    #[test]
    fn deadline_ids_are_unique_only_within_an_actor() {
        let traces = Verifier::<()>::traces(|cfg| {
            cfg.spawn(Fiber::new(|sdk| {
                sdk.sleep_until(sdk.deadline(Duration::from_secs(5)));
            }));
            cfg.spawn(Fiber::new(|sdk| {
                sdk.sleep_until(sdk.deadline(Duration::from_secs(5)));
            }));
        });
        assert_eq!(traces.len(), 1);
        assert_trace!(
            traces[0],
            "SpawnOk(:0)@<> → :0 → Deadline(5s)@<1 0>",
            "DeadlineOk(Deadline { id: 0 })@<> → :0 → SleepUntil(Deadline { id: 0 })@<2 0>",
            "SleepUntilOk@<> → :0 → Exit@<3 0>",
            "SpawnOk(:1)@<> → :1 → Deadline(5s)@<0 1>",
            // The following has the same ID.
            "DeadlineOk(Deadline { id: 0 })@<> → :1 → SleepUntil(Deadline { id: 0 })@<0 2>",
            "SleepUntilOk@<> → :1 → Exit@<0 3>",
        );
    }

    #[test]
    fn reverses_all_reversible_deadline_elapsed() {
        let traces = Verifier::<()>::traces(|cfg| {
            cfg.spawn(Fiber::new(|sdk| {
                sdk.deadline_elapsed(sdk.deadline(Duration::from_secs(5)));
            }));
            cfg.spawn(Fiber::new(|sdk| {
                sdk.deadline_elapsed(sdk.deadline(Duration::from_secs(5)));
            }));
        });
        assert_eq!(traces.len(), 4);
        assert_trace!(
            traces[0],
            "SpawnOk(:0)@<> → :0 → Deadline(5s)@<1 0>",
            "DeadlineOk(Deadline { id: 0 })@<> → :0 → DeadlineElapsed(Deadline { id: 0 })@<2 0>",
            "DeadlineElapsedOk(false)@<> → :0 → Exit@<3 0>",
            "SpawnOk(:1)@<> → :1 → Deadline(5s)@<0 1>",
            "DeadlineOk(Deadline { id: 0 })@<> → :1 → DeadlineElapsed(Deadline { id: 0 })@<0 2>",
            "DeadlineElapsedOk(false)@<> → :1 → Exit@<0 3>",
        );
        assert_trace!(
            traces[1],
            "SpawnOk(:0)@<> → :0 → Deadline(5s)@<1 0>",
            "DeadlineOk(Deadline { id: 0 })@<> → :0 → DeadlineElapsed(Deadline { id: 0 })@<2 0>",
            "DeadlineElapsedOk(false)@<> → :0 → Exit@<3 0>",
            "SpawnOk(:1)@<> → :1 → Deadline(5s)@<0 1>",
            "DeadlineOk(Deadline { id: 0 })@<> → :1 → DeadlineElapsed(Deadline { id: 0 })@<0 2>",
            "DeadlineElapsedOk(true)@<> → :1 → Exit@<0 3>",
        );
        assert_trace!(
            traces[2],
            "SpawnOk(:0)@<> → :0 → Deadline(5s)@<1 0>",
            "DeadlineOk(Deadline { id: 0 })@<> → :0 → DeadlineElapsed(Deadline { id: 0 })@<2 0>",
            "DeadlineElapsedOk(true)@<> → :0 → Exit@<3 0>",
            "SpawnOk(:1)@<> → :1 → Deadline(5s)@<0 1>",
            "DeadlineOk(Deadline { id: 0 })@<> → :1 → DeadlineElapsed(Deadline { id: 0 })@<0 2>",
            "DeadlineElapsedOk(false)@<> → :1 → Exit@<0 3>",
        );
        assert_trace!(
            traces[3],
            "SpawnOk(:0)@<> → :0 → Deadline(5s)@<1 0>",
            "DeadlineOk(Deadline { id: 0 })@<> → :0 → DeadlineElapsed(Deadline { id: 0 })@<2 0>",
            "DeadlineElapsedOk(true)@<> → :0 → Exit@<3 0>",
            "SpawnOk(:1)@<> → :1 → Deadline(5s)@<0 1>",
            "DeadlineOk(Deadline { id: 0 })@<> → :1 → DeadlineElapsed(Deadline { id: 0 })@<0 2>",
            "DeadlineElapsedOk(true)@<> → :1 → Exit@<0 3>",
        );
    }

    #[test]
    fn does_not_reverse_known_elapsed_deadline() {
        let traces = Verifier::<()>::traces(|cfg| {
            cfg.spawn(Fiber::new(|sdk| {
                let deadline = sdk.deadline(Duration::from_secs(5));
                sdk.deadline_elapsed(deadline);
                sdk.deadline_elapsed(deadline);
            }));
            cfg.spawn(Fiber::new(|sdk| {
                sdk.deadline_elapsed(sdk.deadline(Duration::from_secs(5)));
            }));
        });
        assert_eq!(traces.len(), 6);
        assert_trace!(
            traces[0],
            "SpawnOk(:0)@<> → :0 → Deadline(5s)@<1 0>",
            "DeadlineOk(Deadline { id: 0 })@<> → :0 → DeadlineElapsed(Deadline { id: 0 })@<2 0>",
            "DeadlineElapsedOk(false)@<> → :0 → DeadlineElapsed(Deadline { id: 0 })@<3 0>",
            "DeadlineElapsedOk(false)@<> → :0 → Exit@<4 0>",
            "SpawnOk(:1)@<> → :1 → Deadline(5s)@<0 1>",
            "DeadlineOk(Deadline { id: 0 })@<> → :1 → DeadlineElapsed(Deadline { id: 0 })@<0 2>",
            "DeadlineElapsedOk(false)@<> → :1 → Exit@<0 3>",
        );
        assert_trace!(
            traces[1],
            "SpawnOk(:0)@<> → :0 → Deadline(5s)@<1 0>",
            "DeadlineOk(Deadline { id: 0 })@<> → :0 → DeadlineElapsed(Deadline { id: 0 })@<2 0>",
            "DeadlineElapsedOk(false)@<> → :0 → DeadlineElapsed(Deadline { id: 0 })@<3 0>",
            "DeadlineElapsedOk(false)@<> → :0 → Exit@<4 0>",
            "SpawnOk(:1)@<> → :1 → Deadline(5s)@<0 1>",
            "DeadlineOk(Deadline { id: 0 })@<> → :1 → DeadlineElapsed(Deadline { id: 0 })@<0 2>",
            "DeadlineElapsedOk(true)@<> → :1 → Exit@<0 3>",
        );
        assert_trace!(
            traces[2],
            "SpawnOk(:0)@<> → :0 → Deadline(5s)@<1 0>",
            "DeadlineOk(Deadline { id: 0 })@<> → :0 → DeadlineElapsed(Deadline { id: 0 })@<2 0>",
            "DeadlineElapsedOk(false)@<> → :0 → DeadlineElapsed(Deadline { id: 0 })@<3 0>",
            "DeadlineElapsedOk(true)@<> → :0 → Exit@<4 0>",
            "SpawnOk(:1)@<> → :1 → Deadline(5s)@<0 1>",
            "DeadlineOk(Deadline { id: 0 })@<> → :1 → DeadlineElapsed(Deadline { id: 0 })@<0 2>",
            "DeadlineElapsedOk(false)@<> → :1 → Exit@<0 3>",
        );
        assert_trace!(
            traces[3],
            "SpawnOk(:0)@<> → :0 → Deadline(5s)@<1 0>",
            "DeadlineOk(Deadline { id: 0 })@<> → :0 → DeadlineElapsed(Deadline { id: 0 })@<2 0>",
            "DeadlineElapsedOk(false)@<> → :0 → DeadlineElapsed(Deadline { id: 0 })@<3 0>",
            "DeadlineElapsedOk(true)@<> → :0 → Exit@<4 0>",
            "SpawnOk(:1)@<> → :1 → Deadline(5s)@<0 1>",
            "DeadlineOk(Deadline { id: 0 })@<> → :1 → DeadlineElapsed(Deadline { id: 0 })@<0 2>",
            "DeadlineElapsedOk(true)@<> → :1 → Exit@<0 3>",
        );
        assert_trace!(
            traces[4],
            "SpawnOk(:0)@<> → :0 → Deadline(5s)@<1 0>",
            "DeadlineOk(Deadline { id: 0 })@<> → :0 → DeadlineElapsed(Deadline { id: 0 })@<2 0>",
            "DeadlineElapsedOk(true)@<> → :0 → DeadlineElapsed(Deadline { id: 0 })@<3 0>",
            "DeadlineElapsedOk(true)@<> → :0 → Exit@<4 0>",
            "SpawnOk(:1)@<> → :1 → Deadline(5s)@<0 1>",
            "DeadlineOk(Deadline { id: 0 })@<> → :1 → DeadlineElapsed(Deadline { id: 0 })@<0 2>",
            "DeadlineElapsedOk(false)@<> → :1 → Exit@<0 3>",
        );
        assert_trace!(
            traces[5],
            "SpawnOk(:0)@<> → :0 → Deadline(5s)@<1 0>",
            "DeadlineOk(Deadline { id: 0 })@<> → :0 → DeadlineElapsed(Deadline { id: 0 })@<2 0>",
            "DeadlineElapsedOk(true)@<> → :0 → DeadlineElapsed(Deadline { id: 0 })@<3 0>",
            "DeadlineElapsedOk(true)@<> → :0 → Exit@<4 0>",
            "SpawnOk(:1)@<> → :1 → Deadline(5s)@<0 1>",
            "DeadlineOk(Deadline { id: 0 })@<> → :1 → DeadlineElapsed(Deadline { id: 0 })@<0 2>",
            "DeadlineElapsedOk(true)@<> → :1 → Exit@<0 3>",
        );
    }

    #[test]
    fn distinguishes_between_elapsed_deadlines() {
        // e.g. when the 1st deadline elapses, it doesn't imply that the second one does as well.
        // This is to guard against a bug I encountered during initial feature development that
        // mixed up the deadlines.
        let traces = Verifier::<()>::traces(|cfg| {
            cfg.spawn(Fiber::new(|sdk| {
                let deadline1 = sdk.deadline(Duration::from_secs(5));
                let deadline2 = sdk.deadline(Duration::from_secs(15));
                sdk.deadline_elapsed(deadline1);
                sdk.deadline_elapsed(deadline2);
            }));
        });
        assert_eq!(traces.len(), 4);
        assert_trace!(
            traces[0],
            "SpawnOk(:0)@<> → :0 → Deadline(5s)@<1>",
            "DeadlineOk(Deadline { id: 0 })@<> → :0 → Deadline(15s)@<2>",
            "DeadlineOk(Deadline { id: 1 })@<> → :0 → DeadlineElapsed(Deadline { id: 0 })@<3>",
            "DeadlineElapsedOk(false)@<> → :0 → DeadlineElapsed(Deadline { id: 1 })@<4>",
            "DeadlineElapsedOk(false)@<> → :0 → Exit@<5>",
        );
        assert_trace!(
            traces[1],
            "SpawnOk(:0)@<> → :0 → Deadline(5s)@<1>",
            "DeadlineOk(Deadline { id: 0 })@<> → :0 → Deadline(15s)@<2>",
            "DeadlineOk(Deadline { id: 1 })@<> → :0 → DeadlineElapsed(Deadline { id: 0 })@<3>",
            "DeadlineElapsedOk(false)@<> → :0 → DeadlineElapsed(Deadline { id: 1 })@<4>",
            "DeadlineElapsedOk(true)@<> → :0 → Exit@<5>",
        );
        assert_trace!(
            traces[2],
            "SpawnOk(:0)@<> → :0 → Deadline(5s)@<1>",
            "DeadlineOk(Deadline { id: 0 })@<> → :0 → Deadline(15s)@<2>",
            "DeadlineOk(Deadline { id: 1 })@<> → :0 → DeadlineElapsed(Deadline { id: 0 })@<3>",
            "DeadlineElapsedOk(true)@<> → :0 → DeadlineElapsed(Deadline { id: 1 })@<4>",
            "DeadlineElapsedOk(false)@<> → :0 → Exit@<5>",
        );
        assert_trace!(
            traces[3],
            "SpawnOk(:0)@<> → :0 → Deadline(5s)@<1>",
            "DeadlineOk(Deadline { id: 0 })@<> → :0 → Deadline(15s)@<2>",
            "DeadlineOk(Deadline { id: 1 })@<> → :0 → DeadlineElapsed(Deadline { id: 0 })@<3>",
            "DeadlineElapsedOk(true)@<> → :0 → DeadlineElapsed(Deadline { id: 1 })@<4>",
            "DeadlineElapsedOk(true)@<> → :0 → Exit@<5>",
        );
    }
}
