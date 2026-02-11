# 2026-02-11 AGENT-002 step history record

- Step history is now stored as a single append-only `StepRecord` with:
  - action input (`Action`)
  - validation outcome (`ValidationOutcome` with errors)
  - step result (`StepResult`, representing apply result when apply ran)
  - timings (`StepTimings`: step/llm/apply/snapshot durations)
- Per-step logs emit validation + timing fields alongside action/result.
- Summary logs include history-derived counts (validation failures, apply failures/successes, done steps).
