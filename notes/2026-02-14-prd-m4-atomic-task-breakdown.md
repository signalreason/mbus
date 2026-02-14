# 2026-02-14 PRD M4 atomic task breakdown

- Replaced `prd.json` content with Milestone 4-only tasks, split into the smallest units that still deliver independent value.
- The new backlog covers screenshot artifact flow, multimodal prompt wiring, two-axis escalation policy, regression fixtures/tests, and optional visual diff + OCR spike work.
- Every task definition of done now explicitly includes passing tests and lint/format checks:
  - `cargo test`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo fmt --all -- --check`
- Validated `/Users/xwoj/src/mbus/prd.json` against `/Users/xwoj/src/lever/prd.schema.json` using a temporary Python venv + `jsonschema` module; result: `VALID`.
