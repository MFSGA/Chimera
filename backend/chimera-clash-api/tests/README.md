# Chimera Clash API tests

The default crate suite runs deterministic decoding, serialization, REST, stream,
retry, Specta and transport tests against local fixtures and mock servers:

```powershell
cargo test --manifest-path backend/Cargo.toml -p chimera-clash-api
```

`connections_decode.rs` checks the connection wire contract, including unknown
fields, duplicate keys, missing/null values and port bounds. The scoped
`chimera_serde` derives preserve the reference decoder's typed-field and extra
field behavior; see [chimera-serde](../../chimera-serde/README.md).

Allocation and parse-time probes are available as examples:

```powershell
cargo run --manifest-path backend/Cargo.toml -p chimera-clash-api --features allocation-probes --example connections_allocations
cargo run --manifest-path backend/Cargo.toml -p chimera-clash-api --features allocation-probes --example websocket_text_allocations
cargo run --release --manifest-path backend/Cargo.toml -p chimera-clash-api --example connections_timing
```

`mihomo.rs` is an ignored real-core integration test. It uses HTTP plus named
pipe on Windows or Unix socket on Unix. Provide a local Mihomo executable through
`MIHOMO_BIN` before running it; it creates an isolated temporary core home.
This host-dependent test is not part of the default suite:

```powershell
$env:MIHOMO_BIN = 'C:\path\to\mihomo.exe'
cargo test --manifest-path backend/Cargo.toml -p chimera-clash-api --test mihomo -- --ignored
```

The test exercises the public REST and stream methods against Mihomo. A passing
mock-server test proves request/response behavior only; it does not prove the
real-core transport matrix unless the ignored test is actually run.
