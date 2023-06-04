use {fibril::Fiber, fibril_verifier::Verifier};

fn main() {
    tracing_subscriber::fmt::init();
    let mut rt = fibril::UdpRuntime::new_with_serde_json().port_fn(|n| 3000 + n);
    let verifier_id = rt.spawn(|| {
        Verifier::new(|cfg| {
            let server = cfg.spawn(Fiber::new(|sdk| {
                let mut value = String::new();
                loop {
                    let (src, msg) = sdk.recv();
                    if msg == "READ" {
                        sdk.send(src, value.clone());
                    } else {
                        value = msg;
                        sdk.send(src, "UPDATED".to_string());
                    }
                }
            }));
            cfg.spawn(Fiber::new(move |sdk| {
                sdk.send(server, "A".to_string());
                sdk.recv();
                sdk.send(server, "READ".to_string());
                sdk.recv();
            }));
            cfg.spawn(Fiber::new(move |sdk| {
                sdk.send(server, "READ".to_string());
                sdk.recv();
                sdk.send(server, "B".to_string());
                sdk.recv();
            }));
        })
        .into_fiber()
    });
    println!("Listening at: {verifier_id}");
    println!(
        "You can connect with a UDP client. For instance: nc -u {}",
        format!("{}", verifier_id).replace(":", " ")
    );
    println!("Then: \"Help\"");
    rt.join().unwrap();
}
