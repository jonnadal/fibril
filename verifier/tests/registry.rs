use {
    fibril::{Fiber, Id, Sdk},
    fibril_verifier::{assert_trace, RunResult, TraceRecordingVisitor, Verifier},
    std::collections::BTreeSet,
};

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
enum Msg {
    Register(Id),
    RegistryAvailable(Id),
}

fn registry(sdk: Sdk<Msg>) {
    let mut registered = BTreeSet::new();
    loop {
        let (_src, msg) = sdk.recv();
        if let Msg::Register(id) = msg {
            registered.insert(id);
        }
    }
}

fn worker(sdk: Sdk<Msg>) {
    loop {
        if let (_src, Msg::RegistryAvailable(registry)) = sdk.recv() {
            sdk.send(registry, Msg::Register(sdk.id()));
            return; // `sdk.exit()` would work equally well
        }
    }
}

#[test]
fn has_expected_traces() {
    let (record, replay) = TraceRecordingVisitor::new_with_replay();
    let mut verifier = Verifier::new(|cfg| {
        let registry = cfg.spawn(Fiber::new(registry));
        let worker1 = cfg.spawn(Fiber::new(worker));
        let worker2 = cfg.spawn(Fiber::new(worker));
        cfg.spawn(Fiber::new(move |sdk| {
            sdk.send(registry, Msg::Register(sdk.id()));
            sdk.send(worker1, Msg::RegistryAvailable(registry));
            sdk.send(worker2, Msg::RegistryAvailable(registry));
            sdk.exit();
        }));
    })
    .visitor(record);
    assert_eq!(verifier.run(), RunResult::Complete);

    let traces = replay();
    assert_eq!(traces.len(), 6);
    assert_trace![
        traces[0],
        "SpawnOk(:0)@<> → :0 → Recv@<1 0 0 0>",
        "SpawnOk(:1)@<> → :1 → Recv@<0 1 0 0>",
        "SpawnOk(:2)@<> → :2 → Recv@<0 0 1 0>",
        "SpawnOk(:3)@<> → :3 → Send(:0, Register(:3))@<0 0 0 1>",
        "RecvOk(:3, Register(:3))@<0 0 0 1> → :0 → Recv@<2 0 0 1>",
        "SendOk@<> → :3 → Send(:1, RegistryAvailable(:0))@<0 0 0 2>",
        "RecvOk(:3, RegistryAvailable(:0))@<0 0 0 2> → :1 → Send(:0, Register(:1))@<0 2 0 2>",
        "RecvOk(:1, Register(:1))@<0 2 0 2> → :0 → Recv@<3 2 0 2>",
        "SendOk@<> → :1 → Exit@<0 3 0 2>",
        "SendOk@<> → :3 → Send(:2, RegistryAvailable(:0))@<0 0 0 3>",
        "RecvOk(:3, RegistryAvailable(:0))@<0 0 0 3> → :2 → Send(:0, Register(:2))@<0 0 2 3>",
        "RecvOk(:2, Register(:2))@<0 0 2 3> → :0 → Recv@<4 2 2 3>",
        "SendOk@<> → :2 → Exit@<0 0 3 3>",
        "SendOk@<> → :3 → Exit@<0 0 0 4>",
    ];
    assert_trace![
        traces[1],
        "SpawnOk(:0)@<> → :0 → Recv@<1 0 0 0>",
        "SpawnOk(:1)@<> → :1 → Recv@<0 1 0 0>",
        "SpawnOk(:2)@<> → :2 → Recv@<0 0 1 0>",
        "SpawnOk(:3)@<> → :3 → Send(:0, Register(:3))@<0 0 0 1>",
        "RecvOk(:3, Register(:3))@<0 0 0 1> → :0 → Recv@<2 0 0 1>",
        "SendOk@<> → :3 → Send(:1, RegistryAvailable(:0))@<0 0 0 2>",
        "RecvOk(:3, RegistryAvailable(:0))@<0 0 0 2> → :1 → Send(:0, Register(:1))@<0 2 0 2>",
        "SendOk@<> → :3 → Send(:2, RegistryAvailable(:0))@<0 0 0 3>",
        "RecvOk(:3, RegistryAvailable(:0))@<0 0 0 3> → :2 → Send(:0, Register(:2))@<0 0 2 3>",
        "RecvOk(:2, Register(:2))@<0 0 2 3> → :0 → Recv@<3 0 2 3>",
        "RecvOk(:1, Register(:1))@<0 2 0 2> → :0 → Recv@<4 2 2 3>",
        "SendOk@<> → :1 → Exit@<0 3 0 2>",
        "SendOk@<> → :2 → Exit@<0 0 3 3>",
        "SendOk@<> → :3 → Exit@<0 0 0 4>",
    ];
    assert_trace![
        traces[2],
        "SpawnOk(:0)@<> → :0 → Recv@<1 0 0 0>",
        "SpawnOk(:1)@<> → :1 → Recv@<0 1 0 0>",
        "SpawnOk(:2)@<> → :2 → Recv@<0 0 1 0>",
        "SpawnOk(:3)@<> → :3 → Send(:0, Register(:3))@<0 0 0 1>",
        "SendOk@<> → :3 → Send(:1, RegistryAvailable(:0))@<0 0 0 2>",
        "RecvOk(:3, RegistryAvailable(:0))@<0 0 0 2> → :1 → Send(:0, Register(:1))@<0 2 0 2>",
        "RecvOk(:1, Register(:1))@<0 2 0 2> → :0 → Recv@<2 2 0 2>",
        "RecvOk(:3, Register(:3))@<0 0 0 1> → :0 → Recv@<3 2 0 2>",
        "SendOk@<> → :1 → Exit@<0 3 0 2>",
        "SendOk@<> → :3 → Send(:2, RegistryAvailable(:0))@<0 0 0 3>",
        "RecvOk(:3, RegistryAvailable(:0))@<0 0 0 3> → :2 → Send(:0, Register(:2))@<0 0 2 3>",
        "RecvOk(:2, Register(:2))@<0 0 2 3> → :0 → Recv@<4 2 2 3>",
        "SendOk@<> → :2 → Exit@<0 0 3 3>",
        "SendOk@<> → :3 → Exit@<0 0 0 4>",
    ];
    assert_trace![
        traces[3],
        "SpawnOk(:0)@<> → :0 → Recv@<1 0 0 0>",
        "SpawnOk(:1)@<> → :1 → Recv@<0 1 0 0>",
        "SpawnOk(:2)@<> → :2 → Recv@<0 0 1 0>",
        "SpawnOk(:3)@<> → :3 → Send(:0, Register(:3))@<0 0 0 1>",
        "SendOk@<> → :3 → Send(:1, RegistryAvailable(:0))@<0 0 0 2>",
        "RecvOk(:3, RegistryAvailable(:0))@<0 0 0 2> → :1 → Send(:0, Register(:1))@<0 2 0 2>",
        "RecvOk(:1, Register(:1))@<0 2 0 2> → :0 → Recv@<2 2 0 2>",
        "SendOk@<> → :3 → Send(:2, RegistryAvailable(:0))@<0 0 0 3>",
        "RecvOk(:3, RegistryAvailable(:0))@<0 0 0 3> → :2 → Send(:0, Register(:2))@<0 0 2 3>",
        "RecvOk(:2, Register(:2))@<0 0 2 3> → :0 → Recv@<3 2 2 3>",
        "RecvOk(:3, Register(:3))@<0 0 0 1> → :0 → Recv@<4 2 2 3>",
        "SendOk@<> → :1 → Exit@<0 3 0 2>",
        "SendOk@<> → :2 → Exit@<0 0 3 3>",
        "SendOk@<> → :3 → Exit@<0 0 0 4>",
    ];
    assert_trace![
        traces[4],
        "SpawnOk(:0)@<> → :0 → Recv@<1 0 0 0>",
        "SpawnOk(:1)@<> → :1 → Recv@<0 1 0 0>",
        "SpawnOk(:2)@<> → :2 → Recv@<0 0 1 0>",
        "SpawnOk(:3)@<> → :3 → Send(:0, Register(:3))@<0 0 0 1>",
        "SendOk@<> → :3 → Send(:1, RegistryAvailable(:0))@<0 0 0 2>",
        "RecvOk(:3, RegistryAvailable(:0))@<0 0 0 2> → :1 → Send(:0, Register(:1))@<0 2 0 2>",
        "SendOk@<> → :3 → Send(:2, RegistryAvailable(:0))@<0 0 0 3>",
        "RecvOk(:3, RegistryAvailable(:0))@<0 0 0 3> → :2 → Send(:0, Register(:2))@<0 0 2 3>",
        "RecvOk(:2, Register(:2))@<0 0 2 3> → :0 → Recv@<2 0 2 3>",
        "RecvOk(:1, Register(:1))@<0 2 0 2> → :0 → Recv@<3 2 2 3>",
        "RecvOk(:3, Register(:3))@<0 0 0 1> → :0 → Recv@<4 2 2 3>",
        "SendOk@<> → :1 → Exit@<0 3 0 2>",
        "SendOk@<> → :2 → Exit@<0 0 3 3>",
        "SendOk@<> → :3 → Exit@<0 0 0 4>",
    ];
    assert_trace![
        traces[5],
        "SpawnOk(:0)@<> → :0 → Recv@<1 0 0 0>",
        "SpawnOk(:1)@<> → :1 → Recv@<0 1 0 0>",
        "SpawnOk(:2)@<> → :2 → Recv@<0 0 1 0>",
        "SpawnOk(:3)@<> → :3 → Send(:0, Register(:3))@<0 0 0 1>",
        "SendOk@<> → :3 → Send(:1, RegistryAvailable(:0))@<0 0 0 2>",
        "RecvOk(:3, RegistryAvailable(:0))@<0 0 0 2> → :1 → Send(:0, Register(:1))@<0 2 0 2>",
        "SendOk@<> → :3 → Send(:2, RegistryAvailable(:0))@<0 0 0 3>",
        "RecvOk(:3, RegistryAvailable(:0))@<0 0 0 3> → :2 → Send(:0, Register(:2))@<0 0 2 3>",
        "RecvOk(:2, Register(:2))@<0 0 2 3> → :0 → Recv@<2 0 2 3>",
        "RecvOk(:3, Register(:3))@<0 0 0 1> → :0 → Recv@<3 0 2 3>",
        "RecvOk(:1, Register(:1))@<0 2 0 2> → :0 → Recv@<4 2 2 3>",
        "SendOk@<> → :1 → Exit@<0 3 0 2>",
        "SendOk@<> → :2 → Exit@<0 0 3 3>",
        "SendOk@<> → :3 → Exit@<0 0 0 4>",
    ];
}
