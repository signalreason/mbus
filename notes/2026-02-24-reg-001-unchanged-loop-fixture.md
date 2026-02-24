# 2026-02-24 REG-001 unchanged-state loop fixture

- Added an integration test that drives the agent loop with deterministic `Wait` actions
  against the local harness page to keep `state_hash` unchanged across steps.
- Test asserts `RunStatus::NoProgress`, equal `state_hash` values across steps, and
  monotonic trigger counters (`state_hash_streak`, `no_progress`) to capture loop behavior.
- Fixture avoids repeat-action validation by varying wait durations while keeping page state
  stable, making it reliable in CI.
- Added a dedicated static harness page (`harness/pages/loop.html`) and `/loop` route so the
  fixture does not depend on dynamic form elements or focus changes.
