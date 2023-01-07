use {
    fibril::{Fiber, Id, Sdk},
    fibril_verifier::{assert_trace, RunResult, TraceRecordingVisitor, Verifier},
};

fn server(sdk: Sdk<String>) {
    loop {
        let (src, msg) = sdk.recv();
        sdk.send(src, msg);
    }
}

fn new_client(server: Id) -> impl Fn(Sdk<String>) {
    move |sdk| {
        let mut count = 1;
        loop {
            let req = format!("Message{count}");
            sdk.send(server, req.clone());
            if sdk.recv() != (server, req) {
                continue;
            }
            count += 1;
            if count == 3 {
                sdk.exit();
            }
        }
    }
}

#[test]
fn has_expected_traces() {
    let (record, replay) = TraceRecordingVisitor::new_with_replay();
    let mut verifier = Verifier::new(|cfg| {
        let server = cfg.spawn(Fiber::new(server));
        cfg.spawn(Fiber::new(new_client(server)));
        cfg.spawn(Fiber::new(new_client(server)));
    })
    .visitor(record);
    assert_eq!(verifier.run(), RunResult::Complete);

    let traces = replay();
    assert_eq!(traces.len(), 6);
    assert_trace![
        traces[0],
        "SpawnOk(:0)@<> → :0 → Recv@<1 0 0>",
        "SpawnOk(:1)@<> → :1 → Send(:0, \"Message1\")@<0 1 0>",
        "RecvOk(:1, \"Message1\")@<0 1 0> → :0 → Send(:1, \"Message1\")@<2 1 0>",
        "SendOk@<> → :0 → Recv@<3 1 0>",
        "SendOk@<> → :1 → Recv@<0 2 0>",
        "RecvOk(:0, \"Message1\")@<2 1 0> → :1 → Send(:0, \"Message2\")@<2 3 0>",
        "RecvOk(:1, \"Message2\")@<2 3 0> → :0 → Send(:1, \"Message2\")@<4 3 0>",
        "SendOk@<> → :0 → Recv@<5 3 0>",
        "SendOk@<> → :1 → Recv@<2 4 0>",
        "RecvOk(:0, \"Message2\")@<4 3 0> → :1 → Exit@<4 5 0>",
        "SpawnOk(:2)@<> → :2 → Send(:0, \"Message1\")@<0 0 1>",
        "RecvOk(:2, \"Message1\")@<0 0 1> → :0 → Send(:2, \"Message1\")@<6 3 1>",
        "SendOk@<> → :0 → Recv@<7 3 1>",
        "SendOk@<> → :2 → Recv@<0 0 2>",
        "RecvOk(:0, \"Message1\")@<6 3 1> → :2 → Send(:0, \"Message2\")@<6 3 3>",
        "RecvOk(:2, \"Message2\")@<6 3 3> → :0 → Send(:2, \"Message2\")@<8 3 3>",
        "SendOk@<> → :0 → Recv@<9 3 3>",
        "SendOk@<> → :2 → Recv@<6 3 4>",
        "RecvOk(:0, \"Message2\")@<8 3 3> → :2 → Exit@<8 3 5>",
    ];
    assert_trace![
        traces[1],
        "SpawnOk(:0)@<> → :0 → Recv@<1 0 0>",
        "SpawnOk(:1)@<> → :1 → Send(:0, \"Message1\")@<0 1 0>",
        "RecvOk(:1, \"Message1\")@<0 1 0> → :0 → Send(:1, \"Message1\")@<2 1 0>",
        "SendOk@<> → :0 → Recv@<3 1 0>",
        "SendOk@<> → :1 → Recv@<0 2 0>",
        "RecvOk(:0, \"Message1\")@<2 1 0> → :1 → Send(:0, \"Message2\")@<2 3 0>",
        "SpawnOk(:2)@<> → :2 → Send(:0, \"Message1\")@<0 0 1>",
        "RecvOk(:2, \"Message1\")@<0 0 1> → :0 → Send(:2, \"Message1\")@<4 1 1>",
        "SendOk@<> → :0 → Recv@<5 1 1>",
        "RecvOk(:1, \"Message2\")@<2 3 0> → :0 → Send(:1, \"Message2\")@<6 3 1>",
        "SendOk@<> → :0 → Recv@<7 3 1>",
        "SendOk@<> → :1 → Recv@<2 4 0>",
        "RecvOk(:0, \"Message2\")@<6 3 1> → :1 → Exit@<6 5 1>",
        "SendOk@<> → :2 → Recv@<0 0 2>",
        "RecvOk(:0, \"Message1\")@<4 1 1> → :2 → Send(:0, \"Message2\")@<4 1 3>",
        "RecvOk(:2, \"Message2\")@<4 1 3> → :0 → Send(:2, \"Message2\")@<8 3 3>",
        "SendOk@<> → :0 → Recv@<9 3 3>",
        "SendOk@<> → :2 → Recv@<4 1 4>",
        "RecvOk(:0, \"Message2\")@<8 3 3> → :2 → Exit@<8 3 5>",
    ];
    assert_trace![
        traces[2],
        "SpawnOk(:0)@<> → :0 → Recv@<1 0 0>",
        "SpawnOk(:1)@<> → :1 → Send(:0, \"Message1\")@<0 1 0>",
        "RecvOk(:1, \"Message1\")@<0 1 0> → :0 → Send(:1, \"Message1\")@<2 1 0>",
        "SendOk@<> → :0 → Recv@<3 1 0>",
        "SendOk@<> → :1 → Recv@<0 2 0>",
        "RecvOk(:0, \"Message1\")@<2 1 0> → :1 → Send(:0, \"Message2\")@<2 3 0>",
        "SpawnOk(:2)@<> → :2 → Send(:0, \"Message1\")@<0 0 1>",
        "RecvOk(:2, \"Message1\")@<0 0 1> → :0 → Send(:2, \"Message1\")@<4 1 1>",
        "SendOk@<> → :0 → Recv@<5 1 1>",
        "SendOk@<> → :2 → Recv@<0 0 2>",
        "RecvOk(:0, \"Message1\")@<4 1 1> → :2 → Send(:0, \"Message2\")@<4 1 3>",
        "RecvOk(:2, \"Message2\")@<4 1 3> → :0 → Send(:2, \"Message2\")@<6 1 3>",
        "SendOk@<> → :0 → Recv@<7 1 3>",
        "RecvOk(:1, \"Message2\")@<2 3 0> → :0 → Send(:1, \"Message2\")@<8 3 3>",
        "SendOk@<> → :0 → Recv@<9 3 3>",
        "SendOk@<> → :1 → Recv@<2 4 0>",
        "RecvOk(:0, \"Message2\")@<8 3 3> → :1 → Exit@<8 5 3>",
        "SendOk@<> → :2 → Recv@<4 1 4>",
        "RecvOk(:0, \"Message2\")@<6 1 3> → :2 → Exit@<6 1 5>",
    ];
    assert_trace![
        traces[3],
        "SpawnOk(:0)@<> → :0 → Recv@<1 0 0>",
        "SpawnOk(:1)@<> → :1 → Send(:0, \"Message1\")@<0 1 0>",
        "SpawnOk(:2)@<> → :2 → Send(:0, \"Message1\")@<0 0 1>",
        "RecvOk(:2, \"Message1\")@<0 0 1> → :0 → Send(:2, \"Message1\")@<2 0 1>",
        "SendOk@<> → :0 → Recv@<3 0 1>",
        "RecvOk(:1, \"Message1\")@<0 1 0> → :0 → Send(:1, \"Message1\")@<4 1 1>",
        "SendOk@<> → :0 → Recv@<5 1 1>",
        "SendOk@<> → :1 → Recv@<0 2 0>",
        "RecvOk(:0, \"Message1\")@<4 1 1> → :1 → Send(:0, \"Message2\")@<4 3 1>",
        "RecvOk(:1, \"Message2\")@<4 3 1> → :0 → Send(:1, \"Message2\")@<6 3 1>",
        "SendOk@<> → :0 → Recv@<7 3 1>",
        "SendOk@<> → :1 → Recv@<4 4 1>",
        "RecvOk(:0, \"Message2\")@<6 3 1> → :1 → Exit@<6 5 1>",
        "SendOk@<> → :2 → Recv@<0 0 2>",
        "RecvOk(:0, \"Message1\")@<2 0 1> → :2 → Send(:0, \"Message2\")@<2 0 3>",
        "RecvOk(:2, \"Message2\")@<2 0 3> → :0 → Send(:2, \"Message2\")@<8 3 3>",
        "SendOk@<> → :0 → Recv@<9 3 3>",
        "SendOk@<> → :2 → Recv@<2 0 4>",
        "RecvOk(:0, \"Message2\")@<8 3 3> → :2 → Exit@<8 3 5>",
    ];
    assert_trace![
        traces[4],
        "SpawnOk(:0)@<> → :0 → Recv@<1 0 0>",
        "SpawnOk(:1)@<> → :1 → Send(:0, \"Message1\")@<0 1 0>",
        "SpawnOk(:2)@<> → :2 → Send(:0, \"Message1\")@<0 0 1>",
        "RecvOk(:2, \"Message1\")@<0 0 1> → :0 → Send(:2, \"Message1\")@<2 0 1>",
        "SendOk@<> → :0 → Recv@<3 0 1>",
        "RecvOk(:1, \"Message1\")@<0 1 0> → :0 → Send(:1, \"Message1\")@<4 1 1>",
        "SendOk@<> → :0 → Recv@<5 1 1>",
        "SendOk@<> → :1 → Recv@<0 2 0>",
        "RecvOk(:0, \"Message1\")@<4 1 1> → :1 → Send(:0, \"Message2\")@<4 3 1>",
        "SendOk@<> → :2 → Recv@<0 0 2>",
        "RecvOk(:0, \"Message1\")@<2 0 1> → :2 → Send(:0, \"Message2\")@<2 0 3>",
        "RecvOk(:2, \"Message2\")@<2 0 3> → :0 → Send(:2, \"Message2\")@<6 1 3>",
        "SendOk@<> → :0 → Recv@<7 1 3>",
        "RecvOk(:1, \"Message2\")@<4 3 1> → :0 → Send(:1, \"Message2\")@<8 3 3>",
        "SendOk@<> → :0 → Recv@<9 3 3>",
        "SendOk@<> → :1 → Recv@<4 4 1>",
        "RecvOk(:0, \"Message2\")@<8 3 3> → :1 → Exit@<8 5 3>",
        "SendOk@<> → :2 → Recv@<2 0 4>",
        "RecvOk(:0, \"Message2\")@<6 1 3> → :2 → Exit@<6 1 5>",
    ];
    assert_trace![
        traces[5],
        "SpawnOk(:0)@<> → :0 → Recv@<1 0 0>",
        "SpawnOk(:1)@<> → :1 → Send(:0, \"Message1\")@<0 1 0>",
        "SpawnOk(:2)@<> → :2 → Send(:0, \"Message1\")@<0 0 1>",
        "RecvOk(:2, \"Message1\")@<0 0 1> → :0 → Send(:2, \"Message1\")@<2 0 1>",
        "SendOk@<> → :0 → Recv@<3 0 1>",
        "SendOk@<> → :2 → Recv@<0 0 2>",
        "RecvOk(:0, \"Message1\")@<2 0 1> → :2 → Send(:0, \"Message2\")@<2 0 3>",
        "RecvOk(:2, \"Message2\")@<2 0 3> → :0 → Send(:2, \"Message2\")@<4 0 3>",
        "SendOk@<> → :0 → Recv@<5 0 3>",
        "RecvOk(:1, \"Message1\")@<0 1 0> → :0 → Send(:1, \"Message1\")@<6 1 3>",
        "SendOk@<> → :0 → Recv@<7 1 3>",
        "SendOk@<> → :1 → Recv@<0 2 0>",
        "RecvOk(:0, \"Message1\")@<6 1 3> → :1 → Send(:0, \"Message2\")@<6 3 3>",
        "RecvOk(:1, \"Message2\")@<6 3 3> → :0 → Send(:1, \"Message2\")@<8 3 3>",
        "SendOk@<> → :0 → Recv@<9 3 3>",
        "SendOk@<> → :1 → Recv@<6 4 3>",
        "RecvOk(:0, \"Message2\")@<8 3 3> → :1 → Exit@<8 5 3>",
        "SendOk@<> → :2 → Recv@<2 0 4>",
        "RecvOk(:0, \"Message2\")@<4 0 3> → :2 → Exit@<4 0 5>",
    ];
}
