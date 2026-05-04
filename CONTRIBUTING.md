# Contributing to Rill

First off, thank you for considering contributing to Rill. We welcome
contributions of all kinds — bug reports, feature requests, documentation
improvements, DSP algorithms, audio backends, and more.

## Code of Conduct

This project is governed by the [Contributor Covenant](CODE_OF_CONDUCT.md).
By participating you agree to uphold its terms.

## Where to start

- **Bug reports / feature requests** — open a [GitHub Issue](https://github.com/DigitalRats/rill/issues)
- **Documentation** — check the [mdBook guide](https://rill-adrift.io) and improve it
- **DSP algorithms** — add generators, filters, effects to `rill-core-dsp`
- **Audio backends** — help with ALSA, CoreAudio, WASAPI, JACK, PipeWire
- **Testing** — test on different platforms and with different hardware

## Development workflow

We use [Git Flow](https://github.com/petervanderdoes/gitflow-avh) with
[Conventional Commits](https://www.conventionalcommits.org/).

```bash
# 1. Fork and clone
git clone https://github.com/your-username/rill
cd rill

# 2. Install pre-commit hook (rejects direct commits to develop/main)
./scripts/install-hooks.sh

# 3. Start a feature branch
git flow feature start my-feature

# 4. Make changes, commit, test
cargo test --workspace
cargo clippy --workspace
cargo fmt

# 5. Finish the feature
git flow feature finish my-feature
```

## Code conventions

### Safety & unsafe policy
- `#![deny(unsafe_code)]` is set in 7 crates — **always ask permission** before
  suggesting `unsafe` in those crates
- Prefer existing abstractions (buffers, SIMD wrappers) over raw pointer
  manipulation
- Architectural safety over micro-optimisations unless a bottleneck is proven

### Zero-copy data flow
Data copying across node ports is **forbidden** except in these cases:
1. **Branching (fan-out)** — one source feeds multiple destinations
2. **Accumulation for delay / feedback** — delay lines, feedback loops
3. **External API boundary** — when the external API's buffer layout cannot be
   directly wrapped by a `Port<T, BUF_SIZE>`

### Dependencies
- Do not add new external crates without explicit confirmation
- Prefer internal workspace tools over bringing in new third-party dependencies

### Testing
```bash
cargo test --workspace          # all tests
cargo test -p <crate>           # single crate
cargo test --doc                # doc tests only
```

### Feature flags
Non-default features should be verified:
```bash
cargo build --no-default-features  # minimal
cargo build --all-features         # everything
```

## Pull request process

1. Ensure all tests pass and clippy is clean
2. Add or update tests for new functionality
3. Update documentation (README, doc comments, mdBook) if needed
4. Use conventional commit messages
5. Link to any related issues

## Getting help

- Open a [GitHub Discussion](https://github.com/DigitalRats/rill/discussions)
- Check the [mdBook documentation](https://rill-adrift.io)
- Read the [architecture documentation](docs/architecture.md)
