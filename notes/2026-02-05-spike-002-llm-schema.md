# SPIKE-002 LLM schema round-trip

## What worked
- Implemented a strict action schema parser with structured errors.
- Valid actions parse into typed `Action` variants.
- Malformed actions return error codes and JSON paths.
- Validation covers length and bounds constraints from the PRD.

## Harness
- Spike crate lives in `spikes/llm_schema`.
- Run tests from repo root:
  - `cargo test --manifest-path spikes/llm_schema/Cargo.toml`

## Notes
- Validation options include `allow_insecure` for http/https enforcement on
  navigate actions.
