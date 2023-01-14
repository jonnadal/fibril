
use {
    consistency_model::{LinearizabilityTester, Register, RegisterOp, RegisterRet},
    fibril::{Fiber, Sdk},
    fibril_verifier::{
        assert_trace, ConsistencyClient, RequestId, Synchronous, TraceRecordingVisitor, Verifier,
    },
};

type Value = String;

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
enum Msg {
    Put(RequestId, Value),
    PutOk(RequestId),

    Get(RequestId),
    GetOk(RequestId, String),
}

fn server(sdk: Sdk<Msg>) {
    let mut val = String::new();
    loop {
        let (src, msg) = sdk.recv();
        match msg {
            Msg::Get(req_id) => {
                sdk.send(src, Msg::GetOk(req_id, val.clone()));
            }
            Msg::Put(req_id, new_val) => {
                val = new_val;
                sdk.send(src, Msg::PutOk(req_id));
            }
            _ => (),
        }
    }
}

impl Synchronous<Register<Value>> for Msg {
    fn encode_request(req_id: RequestId, op: &RegisterOp<Value>) -> Self {
        match op {
            RegisterOp::Read => Msg::Get(req_id),
            RegisterOp::Write(new_val) => Msg::Put(req_id, new_val.clone()),
        }
    }
    fn decode_response(&self) -> (RequestId, RegisterRet<Value>) {
        match self {
            Msg::GetOk(req_id, val) => (*req_id, RegisterRet::ReadOk(val.clone())),
            Msg::PutOk(req_id) => (*req_id, RegisterRet::WriteOk),
            _ => unreachable!(),
        }
    }
}

#[test]
fn has_expected_traces() {
    let (record, replay) = TraceRecordingVisitor::new_with_replay();
    let mut verifier = Verifier::new(|cfg| {
        let server = cfg.spawn(Fiber::new(server));
        cfg.spawn(Fiber::new(move |sdk| {
            sdk.send(server, Msg::Put(100.into(), "Hello, World".into()));
            let (src, msg) = sdk.recv();
            assert_eq!(src, server);
            assert_eq!(msg, Msg::PutOk(100.into()));
        }));
        cfg.spawn(Fiber::new(move |sdk| {
            sdk.send(server, Msg::Get(200.into()));
            let (src, msg) = sdk.recv();
            assert_eq!(src, server);
            if let Msg::GetOk(req_id, _val) = msg {
                assert_eq!(req_id, 200.into());
                // `val` on the other hand varies by trace.
            } else {
                panic!("Unexpected message: {msg:?}");
            }
        }));
    })
    .visitor(record);
    verifier.assert_no_panic();

    let traces = replay();
    assert_eq!(traces.len(), 2);
    assert_trace![
        traces[0],
        "SpawnOk(:0)@<> → :0 → Recv@<1 0 0>",
        "SpawnOk(:1)@<> → :1 → Send(:0, Put(RequestId(100), \"Hello, World\"))@<0 1 0>",
        "RecvOk(:1, Put(RequestId(100), \"Hello, World\"))@<0 1 0> → :0 → Send(:1, PutOk(RequestId(100)))@<2 1 0>",
        "SendOk@<> → :0 → Recv@<3 1 0>",
        "SendOk@<> → :1 → Recv@<0 2 0>",
        "RecvOk(:0, PutOk(RequestId(100)))@<2 1 0> → :1 → Exit@<2 3 0>",
        "SpawnOk(:2)@<> → :2 → Send(:0, Get(RequestId(200)))@<0 0 1>",
        "RecvOk(:2, Get(RequestId(200)))@<0 0 1> → :0 → Send(:2, GetOk(RequestId(200), \"Hello, World\"))@<4 1 1>",
        "SendOk@<> → :0 → Recv@<5 1 1>",
        "SendOk@<> → :2 → Recv@<0 0 2>",
        "RecvOk(:0, GetOk(RequestId(200), \"Hello, World\"))@<4 1 1> → :2 → Exit@<4 1 3>",
    ];
    assert_trace![
        traces[1],
        "SpawnOk(:0)@<> → :0 → Recv@<1 0 0>",
        "SpawnOk(:1)@<> → :1 → Send(:0, Put(RequestId(100), \"Hello, World\"))@<0 1 0>",
        "SpawnOk(:2)@<> → :2 → Send(:0, Get(RequestId(200)))@<0 0 1>",
        "RecvOk(:2, Get(RequestId(200)))@<0 0 1> → :0 → Send(:2, GetOk(RequestId(200), \"\"))@<2 0 1>",
        "SendOk@<> → :0 → Recv@<3 0 1>",
        "RecvOk(:1, Put(RequestId(100), \"Hello, World\"))@<0 1 0> → :0 → Send(:1, PutOk(RequestId(100)))@<4 1 1>",
        "SendOk@<> → :0 → Recv@<5 1 1>",
        "SendOk@<> → :1 → Recv@<0 2 0>",
        "RecvOk(:0, PutOk(RequestId(100)))@<4 1 1> → :1 → Exit@<4 3 1>",
        "SendOk@<> → :2 → Recv@<0 0 2>",
        "RecvOk(:0, GetOk(RequestId(200), \"\"))@<2 0 1> → :2 → Exit@<2 0 3>",
    ];
}

#[test]
fn is_not_linearizable() {
    let mut verifier = Verifier::new(|cfg| {
        let server1 = cfg.spawn(Fiber::new(server));
        let server2 = cfg.spawn(Fiber::new(server));
        cfg.spawn(
            ConsistencyClient::new(LinearizabilityTester::new(Register::default()))
                .thread([
                    (server1, RegisterOp::Write("A".into())),
                    (server2, RegisterOp::Read)
                ])
                .thread([
                        (server1, RegisterOp::Write("A".into())),
                        (server2, RegisterOp::Read)
                ])
                .into_behavior(),
        );
    });
    let (msg, trace) = verifier.assert_panic();
    assert!(msg.contains("is_consistent"));
    assert_trace![
        trace,
        "SpawnOk(:0)@<> → :0 → Recv@<1 0 0>",
        "SpawnOk(:1)@<> → :1 → Recv@<0 1 0>",
        "SpawnOk(:2)@<> → :2 → Send(:0, Put(RequestId(100), \"A\"))@<0 0 1>",
        "RecvOk(:2, Put(RequestId(100), \"A\"))@<0 0 1> → :0 → Send(:2, PutOk(RequestId(100)))@<2 0 1>",
        "SendOk@<> → :0 → Recv@<3 0 1>",
        "SendOk@<> → :2 → Send(:0, Put(RequestId(200), \"A\"))@<0 0 2>",
        "RecvOk(:2, Put(RequestId(200), \"A\"))@<0 0 2> → :0 → Send(:2, PutOk(RequestId(200)))@<4 0 2>",
        "SendOk@<> → :2 → Recv@<0 0 3>",
        "RecvOk(:0, PutOk(RequestId(100)))@<2 0 1> → :2 → Send(:1, Get(RequestId(300)))@<2 0 4>",
        "RecvOk(:2, Get(RequestId(300)))@<2 0 4> → :1 → Send(:2, GetOk(RequestId(300), \"\"))@<2 2 4>",
        "SendOk@<> → :2 → Recv@<2 0 5>",
        "RecvOk(:0, PutOk(RequestId(200)))@<4 0 2> → :2 → Recv@<4 0 6>",
        "RecvOk(:1, GetOk(RequestId(300), \"\"))@<2 2 4> → :2 → Panic(\"assertion failed: self.tester.is_consistent()\")@<4 2 7>",
    ];
}
