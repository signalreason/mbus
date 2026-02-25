# 2026-02-25 REG-003 escalation-before-failure regression assertions

- Updated loop regression transition expectations so repeat-validation includes the initial unchanged-state escalation and the later repeat-validation transition before termination.
- Added a lightweight transition tuple alias in `tests/e2e.rs` to satisfy clippy `type_complexity` without weakening assertions.
- Transition sequences remain explicit in expected test artifacts to catch ladder policy drift, and the quality gate now passes.
