# Contributing to MI Drums

Thank you for helping improve MI Drums. Keep changes focused and describe the user problem they
solve. For architecture and setup details, read [DEVELOPMENT.md](DEVELOPMENT.md) and
[ARCHITECTURE.md](ARCHITECTURE.md).

## Development setup

Install Git and a current stable Rust toolchain with Cargo. Windows is the currently supported and
tested development target. A compatible MIDI drum kit and audio output are needed for end-to-end
hardware testing, but not for the automated test suite.

```sh
cargo build --workspace
cargo run -p drums-app
```

Before opening a pull request, run:

```sh
cargo fmt --check
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo doc --workspace --no-deps
```

Use `cargo fmt --all` to apply formatting. Update Rustdoc and Markdown when a change affects public
types, architecture, setup, or user-visible behavior.

## Pull requests

- Keep each pull request cohesive and explain what changed and why.
- Add focused tests for domain behavior where practical.
- Preserve physical-MIDI versus logical-drum separation and callback timestamps.
- Do not commit build output, local configuration, logs, or media without clear redistribution rights.
- State what you tested. If hardware behavior changed, identify the Windows version, MIDI device,
  relevant mapping/event trace, and audio setup used. Do not claim hardware validation that was not run.

## Issues

Use the repository's issue templates for reproducible bugs and feature requests. Search existing
issues first. Include the MI Drums version, Windows version, hardware, reproduction steps, and useful
screenshots or non-sensitive MIDI Monitor details. Report vulnerabilities through
[SECURITY.md](SECURITY.md), not a public issue.

By contributing, you agree to follow [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
