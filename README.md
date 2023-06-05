# Fibril Repository Root

This repository houses a collection of [Rust](https://www.rust-lang.org/)
crates for implementing and verifying distributed systems.

## Crates

| Crate               | Links |
| ------------------- | ----- |
| `consistency_model` | [docs](https://docs.rs/consistency_model)&emsp;[source](crates/consistency_model/) |
| `fibril`            | [docs](https://docs.rs/fibril)&emsp;[source](crates/fibril/) |
| `fibril_core`       | [docs](https://docs.rs/fibril_core)&emsp;[source](crates/fibril_core/) |
| `fibril_verifier`   | [docs](https://docs.rs/fibril_verifier)&emsp;[source](crates/fibril_verifier/) |
| `vector_clock`      | [docs](https://docs.rs/vector_clock)&emsp;[source](crates/vector_clock/) |

## Contributing

Please make sure your code passes tests/clippy, and apply auto-formatting:

1. `cargo test && cargo test --examples`
2. `cargo clippy && cargo clippy --examples`
3. `cargo fmt`

## License

See [`LICENSE-APACHE`](LICENSE-APACHE) and [`LICENSE-MIT`](LICENSE-MIT).
