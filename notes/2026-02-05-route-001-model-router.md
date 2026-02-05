# 2026-02-05 ROUTE-001 model router

- Implemented `llm::router` with configurable thresholds for failure and
  no-progress escalation (defaults: 2 -> mid, 4 -> strong).
- Router tracks failure/no-progress counters independently and selects the
  highest tier; counters reset on progress.
- Added unit tests for escalation thresholds and reset behavior.
