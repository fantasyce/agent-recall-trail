# Contributing

Contributions are welcome through focused issues and pull requests. Before
opening a change, read `AGENTS.md`, the architecture decisions, and the
clean-room independence rules. New behavior requires a failing test first.

Run:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
bash tests/scripts/release-gate.sh
```

Do not include private memories, real credentials, transcripts, proprietary
product source, or fixtures derived from another memory product.
