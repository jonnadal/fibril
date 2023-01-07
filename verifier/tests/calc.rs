use {
    fibril::{Fiber, Sdk},
    fibril_verifier::{assert_trace, TraceRecordingVisitor, Verifier},
};

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
enum Msg {
    Inc,
    Double,
    Value(u64),
}

fn server(sdk: Sdk<Msg>) {
    let mut val = 0;
    loop {
        let (src, msg) = sdk.recv();
        match msg {
            Msg::Inc => {
                val += 1;
                sdk.send(src, Msg::Value(val));
            }
            Msg::Double => {
                val *= 2;
                sdk.send(src, Msg::Value(val));
            }
            _ => sdk.exit(),
        }
    }
}

#[test]
fn has_expected_traces() {
    let (record, replay) = TraceRecordingVisitor::new_with_replay();
    let mut verifier = Verifier::new(|cfg| {
        let server = cfg.spawn(Fiber::new(server));
        cfg.spawn(Fiber::new(move |sdk| {
            sdk.send(server, Msg::Inc);
            let _ = sdk.recv();
        }));
        cfg.spawn(Fiber::new(move |sdk| {
            sdk.send(server, Msg::Double);
            let _ = sdk.recv();
        }));
    })
    .visitor(record);
    verifier.assert_no_panic();

    let traces = replay();
    assert_eq!(traces.len(), 2);
    assert_trace![
        traces[0],
        "SpawnOk(:0)@<> → :0 → Recv@<1 0 0>",
        "SpawnOk(:1)@<> → :1 → Send(:0, Inc)@<0 1 0>",
        "RecvOk(:1, Inc)@<0 1 0> → :0 → Send(:1, Value(1))@<2 1 0>",
        "SendOk@<> → :0 → Recv@<3 1 0>",
        "SendOk@<> → :1 → Recv@<0 2 0>",
        "RecvOk(:0, Value(1))@<2 1 0> → :1 → Exit@<2 3 0>",
        "SpawnOk(:2)@<> → :2 → Send(:0, Double)@<0 0 1>",
        "RecvOk(:2, Double)@<0 0 1> → :0 → Send(:2, Value(2))@<4 1 1>",
        "SendOk@<> → :0 → Recv@<5 1 1>",
        "SendOk@<> → :2 → Recv@<0 0 2>",
        "RecvOk(:0, Value(2))@<4 1 1> → :2 → Exit@<4 1 3>",
    ];
    assert_trace![
        traces[1],
        "SpawnOk(:0)@<> → :0 → Recv@<1 0 0>",
        "SpawnOk(:1)@<> → :1 → Send(:0, Inc)@<0 1 0>",
        "SpawnOk(:2)@<> → :2 → Send(:0, Double)@<0 0 1>",
        "RecvOk(:2, Double)@<0 0 1> → :0 → Send(:2, Value(0))@<2 0 1>",
        "SendOk@<> → :0 → Recv@<3 0 1>",
        "RecvOk(:1, Inc)@<0 1 0> → :0 → Send(:1, Value(1))@<4 1 1>",
        "SendOk@<> → :0 → Recv@<5 1 1>",
        "SendOk@<> → :1 → Recv@<0 2 0>",
        "RecvOk(:0, Value(1))@<4 1 1> → :1 → Exit@<4 3 1>",
        "SendOk@<> → :2 → Recv@<0 0 2>",
        "RecvOk(:0, Value(0))@<2 0 1> → :2 → Exit@<2 0 3>",
    ];
}
