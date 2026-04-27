# rill-server

OSC (Open Sound Control) server and networking for remote control of Rill audio graphs.

## Key components

- **`OscServer`** — async UDP server with address-pattern dispatching
- **OSC types** — `OscMessage`, `OscBundle`, `OscPacket`, `OscType` (Int, Float, String, Blob, Timetag)
- **Encode/decode** — complete OSC packet serialization and parsing
- **Pattern matching** — `*` wildcard support for handler registration
- **`TimeTag`** — NTP-format timetag for bundle scheduling

## Usage

```rust
let mut server = OscServer::bind("127.0.0.1:9000").await?;
server.handle("/audio/volume", |msg, _src| {
    if let Some(OscType::Float(val)) = msg.args.first() {
        println!("volume = {}", val);
    }
});
server.run().await?;
```

## Dependencies

Standalone — no rill-core dependency. Uses `tokio` for async UDP.

## Links

- Repository: <https://github.com/DigitalRats/rill>
- Documentation: <https://docs.rs/rill-server>
