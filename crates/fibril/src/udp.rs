use {
    fibril_core::{Command, Deadline, Event, Expectation, Id, Step},
    std::{
        collections::BTreeMap,
        fmt::Debug,
        net::{Ipv4Addr, SocketAddrV4, UdpSocket},
        thread::JoinHandle,
        time::Duration,
    },
    tokio::{runtime::Runtime as TokioRuntime, sync::mpsc, time::Instant},
    tracing::{debug, error, info, warn},
};

pub struct UdpRuntime<M> {
    // Configuration.
    de: fn(slice: &[u8]) -> Option<M>,
    ip: Ipv4Addr,
    port_fn: Box<dyn Fn(u16) -> u16>,
    ser: fn(msg: M) -> Option<Vec<u8>>,

    // Runtime state.
    handles: Vec<JoinHandle<()>>,
    handles_tokio: Vec<tokio::task::JoinHandle<()>>,
    rt: TokioRuntime,
}

impl UdpRuntime<String> {
    pub fn new_with_serde_string() -> Self {
        UdpRuntime::new(
            |msg| Some(String::into_bytes(msg)),
            |bytes| std::str::from_utf8(bytes).ok().map(|s| s.to_string()),
        )
    }
}

impl<M> UdpRuntime<M> {
    pub fn ipv4(mut self, ip: Ipv4Addr) -> Self {
        self.ip = ip;
        self
    }

    pub fn join(&mut self) -> std::thread::Result<usize> {
        // FIXME: If a "client" behavior panics but a "server" behavior is still running (for instance)
        // then this current implementation will continue to block and not handle the panic. The
        // code instead needs to shut down the runtime if/when *any* task/thread panics.

        // By default, if a tokio task panics, an error is printed to STDERR but doesn't shut down
        // the runtime. That can cause hard-to-debug behavior. For instance, if the task panics in
        // a test, by default Rust captures the output, and the test might pass due to assertions
        // being skipped. This code guards against that by blocking on completion and propagating
        // the panic.
        let results = self
            .rt
            .block_on(futures::future::join_all(self.handles_tokio.drain(..)));
        for result in results {
            if let Err(e) = result {
                std::panic::resume_unwind(e.into_panic());
            }
        }

        // Errors that arise in the behavior threads are handled differently. Instead they are
        // returned so that the caller can decide whether to panic.
        let count = self.handles.len();
        for handle in self.handles.drain(..) {
            if let Err(e) = handle.join() {
                let msg = "Behavior exited due to panic.";
                if let Some(panic) = e.downcast_ref::<&'static str>() {
                    error!(panic, msg);
                } else if let Some(panic) = e.downcast_ref::<String>() {
                    error!(panic, msg);
                } else {
                    error!(msg);
                }
                return Err(e);
            }
        }
        Ok(count)
    }

    pub fn new(
        ser: fn(msg: M) -> Option<Vec<u8>>,
        de: for<'a> fn(slice: &'a [u8]) -> Option<M>,
    ) -> Self {
        UdpRuntime {
            de,
            ip: Ipv4Addr::LOCALHOST,
            port_fn: Box::new(|_| 0),
            ser,

            handles: Vec::new(),
            handles_tokio: Vec::new(),
            rt: TokioRuntime::new().unwrap(),
        }
    }

    #[cfg(feature = "serde_json")]
    pub fn new_with_serde_json() -> Self
    where
        M: for<'a> serde::Deserialize<'a> + serde::Serialize,
    {
        UdpRuntime::new(
            |msg| serde_json::to_vec(&msg).ok(),
            |slice| serde_json::from_slice(slice).ok(),
        )
    }

    pub fn port_fn(mut self, port_fn: impl Fn(u16) -> u16 + 'static) -> Self {
        self.port_fn = Box::new(port_fn);
        self
    }

    pub fn spawn<S: Step<M> + 'static>(&mut self, behavior: impl Fn() -> S + Send + 'static) -> Id
    where
        M: Debug + Send + 'static,
    {
        let addr = (self.ip, (self.port_fn)(self.handles.len() as u16));
        let socket = UdpSocket::bind(addr).unwrap();
        socket.set_nonblocking(true).unwrap();
        let id = Id::from(socket.local_addr().unwrap());
        info!(?id, "UDP socket bound.");
        let (tx_events, mut rx_events) = mpsc::channel::<Event<M>>(64);
        let (tx_commands, mut rx_commands) = mpsc::channel::<Command<M>>(64);
        if let Err(e) = tx_events.blocking_send(Event::SpawnOk(id)) {
            panic!("Unable to init {}. Error: {:?}", id, e);
        }

        self.handles.push(std::thread::spawn(move || {
            let mut behavior = behavior();
            loop {
                let event = match rx_events.blocking_recv() {
                    None => break,
                    Some(event) => event,
                };
                debug!("{:?} → {}", event, id);
                let command = behavior.step(event);
                debug!(" → {:?}", command);
                if tx_commands.blocking_send(command).is_err() {
                    break;
                }
            }
            info!(?id, "Cleanly interrupted behavior handler for shutdown.");
        }));

        let ser = self.ser;
        let de = self.de;
        self.handles_tokio.push(self.rt.spawn(async move {
            let mut instants = BTreeMap::new();
            let mut next_deadline_id = 0;
            let socket = tokio::net::UdpSocket::from_std(socket).unwrap();
            let mut buf = [0; 256];
            loop {
                let command = match rx_commands.recv().await {
                    None => break,
                    Some(command) => command,
                };
                let next_event = match command {
                    Command::Exit => {
                        return;
                    }
                    Command::Deadline(duration) => {
                        instants.insert(next_deadline_id, Instant::now().checked_add(duration).expect("Invalid duration"));
                        let event = Event::DeadlineOk(Deadline { id: next_deadline_id });
                        next_deadline_id += 1;
                        event
                    }
                    Command::DeadlineElapsed(Deadline { id }) => {
                        let is_elapsed = instants
                            .get(&id).map(|i| i.elapsed() > Duration::ZERO)
                            .unwrap_or(true);
                        // Can use drain_filter once https://github.com/rust-lang/rust/issues/70530
                        // stabilizes.
                        let expired: Vec<_> = instants
                            .iter()
                            .filter_map(|(id, instant)| {
                                (instant.elapsed() > Duration::ZERO).then_some(*id)
                            })
                            .collect();
                        for id in expired {
                            instants.remove(&id);
                        }
                        Event::DeadlineElapsedOk(is_elapsed)
                    }
                    Command::Expect(description) => Event::ExpectOk(Expectation::new(description)),
                    Command::ExpectationMet(_) => Event::ExpectationMetOk,
                    Command::Panic(msg) => {
                        panic!("{}", msg);
                    }
                    Command::Recv => {
                        loop {
                            // FIXME: if a message exceeds the buffer it's truncated
                            let (count, src) = match socket.recv_from(&mut buf).await {
                                Ok((count, src)) => (count, Id::from(src)),
                                Err(err) => panic!(
                                    "Unable to read socket for {}. Crashing. Error: {:?}",
                                    id, err
                                ),
                            };
                            match de(&buf[0..count]) {
                                None => debug!(?src, dst=?id, "Unable to deserialize message. Ignoring."),
                                Some(msg) => break Event::RecvOk(src, msg),
                            }
                        }
                    }
                    Command::Send(dst, msg) => {
                        match ser(msg) {
                            None => warn!(src=?id, ?dst, "Serialization failed. Ignoring."),
                            Some(serialized) => {
                                match socket.send_to(&serialized, SocketAddrV4::from(dst)).await {
                                    Ok(len_sent) => {
                                        if len_sent < serialized.len() {
                                            warn!(src=?id, ?dst, "Message was too large to send. Ignoring.");
                                            continue;
                                        }
                                    }
                                    Err(err) => {
                                        warn!(src=?id, ?dst, ?err, "Unable to write socket. Ignoring.");
                                        continue;
                                    }
                                };
                            }
                        }
                        Event::SendOk
                    }
                    Command::SleepUntil(Deadline { id }) => {
                        if let Some(instant) = instants.get(&id) {
                            tokio::time::sleep_until(*instant).await;
                        }
                        Event::SleepUntilOk
                    }
                    command => panic!("{command:?} is not supported at this time."),
                };
                if tx_events.send(next_event).await.is_err() {
                    info!(?id, "Cleanly interrupted I/O handler for shutdown.");
                    break;
                }
            }
        }));

        id
    }
}

impl Default for UdpRuntime<String> {
    fn default() -> Self {
        Self::new_with_serde_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_run_spawned_behaviors() {
        let mut rt = UdpRuntime::default();
        let server_id = rt.spawn(|| {
            |event| match event {
                Event::SpawnOk(_) => Command::Recv,
                Event::RecvOk(src, msg) => Command::Send(src, msg),
                Event::SendOk => Command::Exit,
                event => panic!("{event:?} was not expected."),
            }
        });
        rt.spawn(move || {
            move |event| match event {
                Event::SpawnOk(_) => Command::Send(server_id, "One".into()),
                Event::SendOk => Command::Recv,
                Event::RecvOk(src, msg) => {
                    assert_eq!(src, server_id);
                    assert_eq!(msg, "One".to_string());
                    Command::Exit
                }
                event => panic!("{event:?} was not expected."),
            }
        });
        rt.join().unwrap();
    }
}
