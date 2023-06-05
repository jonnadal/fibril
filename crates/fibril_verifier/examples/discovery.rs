use {
    fibril::{Deadline, Fiber, Id, Sdk},
    std::{
        collections::{BTreeMap, BTreeSet},
        time::Duration,
    },
    tracing::{info, info_span},
};

#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, serde::Deserialize, serde::Serialize)]
enum Msg {
    Renew,
    Query,
    QueryOk(BTreeSet<Id>),
}

#[tracing::instrument]
fn disco_server(sdk: Sdk<Msg>) {
    const GC_INTERVAL: Duration = Duration::from_secs(5);
    const TTL: Duration = Duration::from_secs(15);

    let mut expirations_by_id: BTreeMap<Id, Deadline> = BTreeMap::new();
    let mut next_gc = sdk.deadline(GC_INTERVAL);
    loop {
        let (client, msg) = sdk.recv();

        // Just in time GC before message handling.
        if sdk.deadline_elapsed(next_gc) {
            info!("Garbage collecting stale records.");
            next_gc = sdk.deadline(GC_INTERVAL);
            expirations_by_id.retain(|_id, expiration| !sdk.deadline_elapsed(*expiration));
        }

        match msg {
            Msg::Renew => {
                expirations_by_id.insert(client, sdk.deadline(TTL));
            }
            Msg::Query => {
                sdk.send(
                    client,
                    Msg::QueryOk(expirations_by_id.keys().cloned().collect()),
                );
            }
            _ => {}
        }
    }
}

fn main() {
    tracing_subscriber::fmt::init();
    let mut rt = fibril::UdpRuntime::new_with_serde_json().port_fn(|n| 3000 + n);
    let server = rt.spawn(|| Fiber::new(disco_server));
    rt.spawn(move || {
        Fiber::new(move |sdk| loop {
            const RENEW_INTERVAL: Duration = Duration::from_secs(3);
            let _span = info_span!("client").entered();
            loop {
                info!("Renewing availability.");
                sdk.send(server, Msg::Renew);
                sdk.sleep_until(sdk.deadline(RENEW_INTERVAL));
            }
        })
    });
    println!("Listening at: {server}");
    println!(
        "You can connect with a UDP client. For instance: nc -u {}",
        format!("{}", server).replace(':', " ")
    );
    println!("Then: \"Query\" or \"Renew\"");
    rt.join().unwrap();
}

#[cfg(test)]
mod test {
    use {super::*, fibril_verifier::*};

    #[test]
    fn has_expected_traces() {
        let (record, replay) = TraceRecordingVisitor::new_with_replay();
        let mut verifier = Verifier::new(|cfg| {
            let server = cfg.spawn(Fiber::new(disco_server));
            cfg.spawn(Fiber::new(move |sdk| {
                sdk.send(server, Msg::Renew);
                sdk.send(server, Msg::Query);
            }));
        })
        .visitor(record);
        verifier.assert_no_panic();

        let traces = replay();
        assert_eq!(traces.len(), 6);
        assert_trace![
            traces[0],
            "SpawnOk(:0)@<> → :0 → Deadline(5s)@<1 0>",
            "DeadlineOk(Deadline { id: 0 })@<> → :0 → Recv@<2 0>",
            "SpawnOk(:1)@<> → :1 → Send(:0, Renew)@<0 1>",
            // Recv Renew. Not ready to GC.
            "RecvOk(:1, Renew)@<0 1> → :0 → DeadlineElapsed(Deadline { id: 0 })@<3 1>",
            "DeadlineElapsedOk(false)@<> → :0 → Deadline(15s)@<4 1>",
            "DeadlineOk(Deadline { id: 1 })@<> → :0 → Recv@<5 1>",
            "SendOk@<> → :1 → Send(:0, Query)@<0 2>",
            // Recv Query. Not ready to GC.
            "RecvOk(:1, Query)@<0 2> → :0 → DeadlineElapsed(Deadline { id: 0 })@<6 2>",
            "DeadlineElapsedOk(false)@<> → :0 → Send(:1, QueryOk({:1}))@<7 2>",
            "SendOk@<> → :0 → Recv@<8 2>",
            "SendOk@<> → :1 → Exit@<0 3>",
        ];
        assert_trace![
            traces[1],
            "SpawnOk(:0)@<> → :0 → Deadline(5s)@<1 0>",
            "DeadlineOk(Deadline { id: 0 })@<> → :0 → Recv@<2 0>",
            "SpawnOk(:1)@<> → :1 → Send(:0, Renew)@<0 1>",
            // Recv Renew. Not ready to GC.
            "RecvOk(:1, Renew)@<0 1> → :0 → DeadlineElapsed(Deadline { id: 0 })@<3 1>",
            "DeadlineElapsedOk(false)@<> → :0 → Deadline(15s)@<4 1>",
            "DeadlineOk(Deadline { id: 1 })@<> → :0 → Recv@<5 1>",
            "SendOk@<> → :1 → Send(:0, Query)@<0 2>",
            // Recv Query. Ready to GC. Nothing eligible.
            "RecvOk(:1, Query)@<0 2> → :0 → DeadlineElapsed(Deadline { id: 0 })@<6 2>",
            "DeadlineElapsedOk(true)@<> → :0 → Deadline(5s)@<7 2>",
            "DeadlineOk(Deadline { id: 2 })@<> → :0 → DeadlineElapsed(Deadline { id: 1 })@<8 2>",
            "DeadlineElapsedOk(false)@<> → :0 → Send(:1, QueryOk({:1}))@<9 2>",
            "SendOk@<> → :0 → Recv@<10 2>",
            "SendOk@<> → :1 → Exit@<0 3>",
        ];
        assert_trace!(
            traces[2],
            "SpawnOk(:0)@<> → :0 → Deadline(5s)@<1 0>",
            "DeadlineOk(Deadline { id: 0 })@<> → :0 → Recv@<2 0>",
            "SpawnOk(:1)@<> → :1 → Send(:0, Renew)@<0 1>",
            // Recv Renew. Not ready to GC.
            "RecvOk(:1, Renew)@<0 1> → :0 → DeadlineElapsed(Deadline { id: 0 })@<3 1>",
            "DeadlineElapsedOk(false)@<> → :0 → Deadline(15s)@<4 1>",
            "DeadlineOk(Deadline { id: 1 })@<> → :0 → Recv@<5 1>",
            "SendOk@<> → :1 → Send(:0, Query)@<0 2>",
            "RecvOk(:1, Query)@<0 2> → :0 → DeadlineElapsed(Deadline { id: 0 })@<6 2>",
            // Recv Query. Ready to GC. Record eligible.
            "DeadlineElapsedOk(true)@<> → :0 → Deadline(5s)@<7 2>",
            "DeadlineOk(Deadline { id: 2 })@<> → :0 → DeadlineElapsed(Deadline { id: 1 })@<8 2>",
            "DeadlineElapsedOk(true)@<> → :0 → Send(:1, QueryOk({}))@<9 2>",
            "SendOk@<> → :0 → Recv@<10 2>",
            "SendOk@<> → :1 → Exit@<0 3>",
        );
        assert_trace!(
            traces[3],
            "SpawnOk(:0)@<> → :0 → Deadline(5s)@<1 0>",
            "DeadlineOk(Deadline { id: 0 })@<> → :0 → Recv@<2 0>",
            "SpawnOk(:1)@<> → :1 → Send(:0, Renew)@<0 1>",
            // Recv Renew. Ready to GC. Nothing present.
            "RecvOk(:1, Renew)@<0 1> → :0 → DeadlineElapsed(Deadline { id: 0 })@<3 1>",
            "DeadlineElapsedOk(true)@<> → :0 → Deadline(5s)@<4 1>",
            "DeadlineOk(Deadline { id: 1 })@<> → :0 → Deadline(15s)@<5 1>",
            "DeadlineOk(Deadline { id: 2 })@<> → :0 → Recv@<6 1>",
            "SendOk@<> → :1 → Send(:0, Query)@<0 2>",
            // Recv Query. Not ready to GC.
            "RecvOk(:1, Query)@<0 2> → :0 → DeadlineElapsed(Deadline { id: 1 })@<7 2>",
            "DeadlineElapsedOk(false)@<> → :0 → Send(:1, QueryOk({:1}))@<8 2>",
            "SendOk@<> → :0 → Recv@<9 2>",
            "SendOk@<> → :1 → Exit@<0 3>",
        );
        assert_trace!(
            traces[4],
            "SpawnOk(:0)@<> → :0 → Deadline(5s)@<1 0>",
            "DeadlineOk(Deadline { id: 0 })@<> → :0 → Recv@<2 0>",
            "SpawnOk(:1)@<> → :1 → Send(:0, Renew)@<0 1>",
            // Recv Renew. Ready to GC. Nothing present.
            "RecvOk(:1, Renew)@<0 1> → :0 → DeadlineElapsed(Deadline { id: 0 })@<3 1>",
            "DeadlineElapsedOk(true)@<> → :0 → Deadline(5s)@<4 1>",
            "DeadlineOk(Deadline { id: 1 })@<> → :0 → Deadline(15s)@<5 1>",
            "DeadlineOk(Deadline { id: 2 })@<> → :0 → Recv@<6 1>",
            "SendOk@<> → :1 → Send(:0, Query)@<0 2>",
            // Recv Query. Ready to GC. Nothing eligible.
            "RecvOk(:1, Query)@<0 2> → :0 → DeadlineElapsed(Deadline { id: 1 })@<7 2>",
            "DeadlineElapsedOk(true)@<> → :0 → Deadline(5s)@<8 2>",
            "DeadlineOk(Deadline { id: 3 })@<> → :0 → DeadlineElapsed(Deadline { id: 2 })@<9 2>",
            "DeadlineElapsedOk(false)@<> → :0 → Send(:1, QueryOk({:1}))@<10 2>",
            "SendOk@<> → :0 → Recv@<11 2>",
            "SendOk@<> → :1 → Exit@<0 3>",
        );
        assert_trace!(
            traces[5],
            "SpawnOk(:0)@<> → :0 → Deadline(5s)@<1 0>",
            "DeadlineOk(Deadline { id: 0 })@<> → :0 → Recv@<2 0>",
            "SpawnOk(:1)@<> → :1 → Send(:0, Renew)@<0 1>",
            // Recv Renew. Ready to GC. Nothing present.
            "RecvOk(:1, Renew)@<0 1> → :0 → DeadlineElapsed(Deadline { id: 0 })@<3 1>",
            "DeadlineElapsedOk(true)@<> → :0 → Deadline(5s)@<4 1>",
            "DeadlineOk(Deadline { id: 1 })@<> → :0 → Deadline(15s)@<5 1>",
            "DeadlineOk(Deadline { id: 2 })@<> → :0 → Recv@<6 1>",
            "SendOk@<> → :1 → Send(:0, Query)@<0 2>",
            // Recv Query. Ready to GC. Record eligible.
            "RecvOk(:1, Query)@<0 2> → :0 → DeadlineElapsed(Deadline { id: 1 })@<7 2>",
            "DeadlineElapsedOk(true)@<> → :0 → Deadline(5s)@<8 2>",
            "DeadlineOk(Deadline { id: 3 })@<> → :0 → DeadlineElapsed(Deadline { id: 2 })@<9 2>",
            "DeadlineElapsedOk(true)@<> → :0 → Send(:1, QueryOk({}))@<10 2>",
            "SendOk@<> → :0 → Recv@<11 2>",
            "SendOk@<> → :1 → Exit@<0 3>",
        );
    }
}
