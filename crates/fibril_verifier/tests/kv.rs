//! Provides a linearizable register "shared memory" abstraction that can serve requests as
//! long as a quorum of actors is available  (e.g. 3 of 5). This code is based on the algorithm
//! described in "[Sharing Memory Robustly in Message-Passing
//! Systems](https://doi.org/10.1145/200836.200869)" by Attiya, Bar-Noy, and Dolev. "ABD"
//! refers to the author names.
//!
//! For a succinct overview of the algorithm, I recommend:
//! http://muratbuffalo.blogspot.com/2012/05/replicatedfault-tolerant-atomic-storage.html

use {
    consistency_model::{LinearizabilityTester, Register, RegisterOp, RegisterRet},
    fibril::{Fiber, Id, Sdk},
    fibril_verifier::{ConsistencyClient, RequestId, Synchronous, TraceRecordingVisitor, Verifier},
    std::collections::BTreeMap,
};

type LogicalClock = (usize, Id);
type Value = String;

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
enum Msg {
    Put(RequestId, Value),
    PutOk(RequestId),

    Get(RequestId),
    GetOk(RequestId, String),

    InternalPut(LogicalClock, Value),
    InternalPutOk,

    InternalGet,
    InternalGetOk(LogicalClock, Value),
}

fn new_servers(cluster_size: usize) -> Vec<Fiber<'static, Msg>> {
    let server_ids: Vec<_> = (0..cluster_size).map(Id::from).collect();
    (0..cluster_size)
        .into_iter()
        .map(|_| {
            let server_ids = server_ids.clone();
            Fiber::new(move |sdk| {
                let id = sdk.id();
                let mut clock = LogicalClock::default();
                let mut value = String::new();
                loop {
                    let (src, msg) = sdk.recv();
                    match msg {
                        Msg::Get(req_id) => {
                            let responses = abd_phase1(&sdk, &server_ids);
                            let (max_clock, max_value) = responses.values().max().unwrap();
                            if !responses.values().all(|(clock, _v)| clock == max_clock) {
                                abd_phase2(&sdk, &server_ids, *max_clock, max_value.clone());
                            }
                            sdk.send(src, Msg::GetOk(req_id, max_value.clone()));
                        }
                        Msg::Put(req_id, new_value) => {
                            let responses = abd_phase1(&sdk, &server_ids);
                            let (max_clock, _max_value) = responses.values().max().unwrap();
                            abd_phase2(&sdk, &server_ids, (max_clock.0 + 1, id), new_value);
                            sdk.send(src, Msg::PutOk(req_id));
                        }
                        Msg::InternalGet => {
                            sdk.send(src, Msg::InternalGetOk(clock, value.clone()));
                        }
                        Msg::InternalPut(new_clock, new_value) => {
                            if new_clock == clock {
                                assert_eq!(new_value, value);
                            }
                            if new_clock > clock {
                                clock = new_clock;
                                value = new_value;
                            }
                            sdk.send(src, Msg::InternalPutOk);
                        }
                        _ => (),
                    }
                }
            })
        })
        .collect()
}

fn abd_phase1(sdk: &Sdk<Msg>, server_ids: &Vec<Id>) -> BTreeMap<Id, (LogicalClock, Value)> {
    sdk.send_broadcast(server_ids.clone(), &Msg::InternalGet);
    sdk.recv_responses(server_ids.len() / 2 + 1, |src, msg| match msg {
        Msg::InternalGetOk(clock, value) => server_ids.contains(&src).then(|| (clock, value)),
        _ => None,
    })
}

fn abd_phase2(sdk: &Sdk<Msg>, server_ids: &Vec<Id>, clock: LogicalClock, value: Value) {
    sdk.send_broadcast(server_ids.clone(), &Msg::InternalPut(clock, value));
    sdk.recv_response_count(server_ids.len() / 2 + 1, |src, msg| {
        server_ids.contains(&src) && msg == Msg::InternalPutOk
    });
}

impl Synchronous<Register<Value>> for Msg {
    fn encode_request(req_id: RequestId, op: &RegisterOp<Value>) -> Self {
        match op {
            RegisterOp::Read => Msg::Get(req_id),
            RegisterOp::Write(new_val) => Msg::Put(req_id, new_val.clone()),
        }
    }
    fn decode_response(self) -> (RequestId, RegisterRet<Value>) {
        match self {
            Msg::GetOk(req_id, val) => (req_id, RegisterRet::ReadOk(val.clone())),
            Msg::PutOk(req_id) => (req_id, RegisterRet::WriteOk),
            _ => unreachable!(),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn does_not_guarantee_client_response() {
        let (record, replay) = TraceRecordingVisitor::new_with_replay();
        let mut verifier = Verifier::new(|cfg| {
            let servers: Vec<_> = new_servers(3).into_iter().map(|s| cfg.spawn(s)).collect();
            cfg.spawn(Fiber::new({
                let servers = servers.clone();
                move |sdk| {
                    sdk.send(servers[0], Msg::Put(100.into(), "A".into()));
                    let expect_response = sdk.expect("receive response");
                    let (src, msg) = sdk.recv();
                    assert_eq!(src, servers[0]);
                    assert_eq!(msg, Msg::PutOk(100.into()));
                    sdk.expectation_met(expect_response);
                }
            }));
            cfg.spawn(Fiber::new({
                let servers = servers.clone();
                move |sdk| {
                    sdk.send(servers[1], Msg::Get(200.into()));
                    let (src, msg) = sdk.recv();
                    assert_eq!(src, servers[1]);
                    if let Msg::GetOk(req_id, _val) = msg {
                        assert_eq!(req_id, 200.into());
                        // `val` on the other hand varies by trace.
                    } else {
                        panic!("Unexpected message: {msg:?}");
                    }
                }
            }));
        })
        .visitor(record);
        verifier.assert_unmet_expectation("receive response");

        let traces = replay();
        assert_eq!(traces.len(), 33);
    }

    #[test]
    fn has_expected_traces() {
        let (record, replay) = TraceRecordingVisitor::new_with_replay();
        let mut verifier = Verifier::new(|cfg| {
            let servers: Vec<_> = new_servers(3).into_iter().map(|s| cfg.spawn(s)).collect();
            cfg.spawn(Fiber::new({
                let servers = servers.clone();
                move |sdk| {
                    sdk.send(servers[0], Msg::Put(100.into(), "A".into()));
                    let (src, msg) = sdk.recv();
                    assert_eq!(src, servers[0]);
                    assert_eq!(msg, Msg::PutOk(100.into()));
                }
            }));
            cfg.spawn(Fiber::new({
                let servers = servers.clone();
                move |sdk| {
                    sdk.send(servers[1], Msg::Get(200.into()));
                    let (src, msg) = sdk.recv();
                    assert_eq!(src, servers[1]);
                    if let Msg::GetOk(req_id, _val) = msg {
                        assert_eq!(req_id, 200.into());
                        // `val` on the other hand varies by trace.
                    } else {
                        panic!("Unexpected message: {msg:?}");
                    }
                }
            }));
        })
        .visitor(record);
        verifier.assert_no_panic();

        let traces = replay();
        assert_eq!(traces.len(), 71);
    }

    #[test]
    #[cfg_attr(debug_assertions, ignore)]
    fn is_linearizable() {
        let (record, replay) = TraceRecordingVisitor::new_with_replay();
        let mut verifier = Verifier::new(|cfg| {
            let servers: Vec<_> = new_servers(3).into_iter().map(|s| cfg.spawn(s)).collect();
            cfg.spawn(Fiber::new(move |sdk| {
                let mut consistency =
                    ConsistencyClient::new(&sdk, LinearizabilityTester::new(Register::default()));
                consistency.invoke(servers[0], RegisterOp::Write("A".into()));
                consistency.invoke(servers[0], RegisterOp::Write("B".into()));
                consistency.await_response();
                consistency.await_response();
                consistency.invoke(servers[1], RegisterOp::Write("C".into()));
                consistency.await_response();
            }));
        })
        .visitor(record);
        verifier.assert_no_panic();

        let traces = replay();
        assert_eq!(traces.len(), 32_860);
    }
}
