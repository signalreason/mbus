# 2026-02-14 DEV-001 quality gate notes

- `cargo test` can hang in parallel when running the e2e suite; using `RUST_TEST_THREADS=1 cargo test` avoided the stall and completed cleanly.
- Clippy (Rust 1.92 toolchain) now flags several patterns (collapsible ifs, derivable Default impls, constant asserts) that needed cleanup to satisfy `-D warnings`.
