# rill-adrift

Umbrella crate re-exporting all [rill](https://github.com/DigitalRats/rill) crates for audio application development.

```toml
[dependencies]
rill-adrift = { path = "../rill/rill-adrift" }
```

```rust
use rill_adrift::prelude::*;
use rill_adrift::rill_oscillators::audio::SineOsc;
```
