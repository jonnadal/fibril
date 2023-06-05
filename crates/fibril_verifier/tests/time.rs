use {
    fibril::{Fiber, Sdk},
    fibril_verifier::{assert_trace, Verifier},
    std::time::Duration,
};

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
enum Msg {
    Request,
    ResponseInvalidRequest,
    ResponseNotReady,
    ResponseOk,
}

fn server(sdk: Sdk<Msg>) {
    let startup_delay = sdk.deadline(Duration::from_secs(10));
    loop {
        let (src, msg) = sdk.recv();
        if msg != Msg::Request {
            sdk.send(src, Msg::ResponseInvalidRequest);
            continue;
        }

        if sdk.deadline_elapsed(startup_delay) {
            sdk.send(src, Msg::ResponseOk);
        } else {
            sdk.send(src, Msg::ResponseNotReady);
        }
    }
}

#[test]
fn simple_example_has_expected_traces() {
    let traces = Verifier::traces(|cfg| {
        let server = cfg.spawn(Fiber::new(server));
        cfg.spawn(Fiber::new(move |sdk| {
            sdk.send(server, Msg::Request);
        }));
    });
    assert_eq!(traces.len(), 2);
    assert_trace![
        traces[0],
        "SpawnOk(:0)@<> → :0 → Deadline(10s)@<1 0>",
        "DeadlineOk(Deadline { id: 0 })@<> → :0 → Recv@<2 0>",
        "SpawnOk(:1)@<> → :1 → Send(:0, Request)@<0 1>",
        "RecvOk(:1, Request)@<0 1> → :0 → DeadlineElapsed(Deadline { id: 0 })@<3 1>",
        "DeadlineElapsedOk(false)@<> → :0 → Send(:1, ResponseNotReady)@<4 1>",
        "SendOk@<> → :0 → Recv@<5 1>",
        "SendOk@<> → :1 → Exit@<0 2>",
    ];
    assert_trace![
        traces[1],
        "SpawnOk(:0)@<> → :0 → Deadline(10s)@<1 0>",
        "DeadlineOk(Deadline { id: 0 })@<> → :0 → Recv@<2 0>",
        "SpawnOk(:1)@<> → :1 → Send(:0, Request)@<0 1>",
        "RecvOk(:1, Request)@<0 1> → :0 → DeadlineElapsed(Deadline { id: 0 })@<3 1>",
        "DeadlineElapsedOk(true)@<> → :0 → Send(:1, ResponseOk)@<4 1>",
        "SendOk@<> → :0 → Recv@<5 1>",
        "SendOk@<> → :1 → Exit@<0 2>",
    ];
}

#[test]
fn more_complex_example_has_expected_traces() {
    let traces = Verifier::traces(|cfg| {
        let server = cfg.spawn(Fiber::new(server));
        cfg.spawn(Fiber::new(move |sdk| {
            sdk.send(server, Msg::Request);
            sdk.send(server, Msg::Request);
        }));
    });
    assert_eq!(traces.len(), 3);
    assert_trace![
        traces[0],
        "SpawnOk(:0)@<> → :0 → Deadline(10s)@<1 0>",
        "DeadlineOk(Deadline { id: 0 })@<> → :0 → Recv@<2 0>",
        "SpawnOk(:1)@<> → :1 → Send(:0, Request)@<0 1>",
        "RecvOk(:1, Request)@<0 1> → :0 → DeadlineElapsed(Deadline { id: 0 })@<3 1>",
        "DeadlineElapsedOk(false)@<> → :0 → Send(:1, ResponseNotReady)@<4 1>",
        "SendOk@<> → :0 → Recv@<5 1>",
        "SendOk@<> → :1 → Send(:0, Request)@<0 2>",
        "RecvOk(:1, Request)@<0 2> → :0 → DeadlineElapsed(Deadline { id: 0 })@<6 2>",
        "DeadlineElapsedOk(false)@<> → :0 → Send(:1, ResponseNotReady)@<7 2>",
        "SendOk@<> → :0 → Recv@<8 2>",
        "SendOk@<> → :1 → Exit@<0 3>",
    ];
    assert_trace![
        traces[1],
        "SpawnOk(:0)@<> → :0 → Deadline(10s)@<1 0>",
        "DeadlineOk(Deadline { id: 0 })@<> → :0 → Recv@<2 0>",
        "SpawnOk(:1)@<> → :1 → Send(:0, Request)@<0 1>",
        "RecvOk(:1, Request)@<0 1> → :0 → DeadlineElapsed(Deadline { id: 0 })@<3 1>",
        "DeadlineElapsedOk(false)@<> → :0 → Send(:1, ResponseNotReady)@<4 1>",
        "SendOk@<> → :0 → Recv@<5 1>",
        "SendOk@<> → :1 → Send(:0, Request)@<0 2>",
        "RecvOk(:1, Request)@<0 2> → :0 → DeadlineElapsed(Deadline { id: 0 })@<6 2>",
        "DeadlineElapsedOk(true)@<> → :0 → Send(:1, ResponseOk)@<7 2>",
        "SendOk@<> → :0 → Recv@<8 2>",
        "SendOk@<> → :1 → Exit@<0 3>",
    ];
    assert_trace![
        traces[2],
        "SpawnOk(:0)@<> → :0 → Deadline(10s)@<1 0>",
        "DeadlineOk(Deadline { id: 0 })@<> → :0 → Recv@<2 0>",
        "SpawnOk(:1)@<> → :1 → Send(:0, Request)@<0 1>",
        "RecvOk(:1, Request)@<0 1> → :0 → DeadlineElapsed(Deadline { id: 0 })@<3 1>",
        "DeadlineElapsedOk(true)@<> → :0 → Send(:1, ResponseOk)@<4 1>",
        "SendOk@<> → :0 → Recv@<5 1>",
        "SendOk@<> → :1 → Send(:0, Request)@<0 2>",
        "RecvOk(:1, Request)@<0 2> → :0 → DeadlineElapsed(Deadline { id: 0 })@<6 2>",
        "DeadlineElapsedOk(true)@<> → :0 → Send(:1, ResponseOk)@<7 2>",
        "SendOk@<> → :0 → Recv@<8 2>",
        "SendOk@<> → :1 → Exit@<0 3>",
    ];
}
