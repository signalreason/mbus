# 2026-02-09 test runtime notes

- `cargo test` in Codex sandbox can fail at `tests/e2e.rs` with `PermissionDenied` when binding a local `TcpListener` (`127.0.0.1:0`).
- Running `cargo test --test e2e` with escalated permissions succeeds (`1 passed`).
- Unit tests and `tests/smoke.rs` pass in sandbox.
- Practical workflow in this environment:
  - Run unit and smoke tests in sandbox.
  - Run E2E with elevated permissions when socket bind is blocked.
