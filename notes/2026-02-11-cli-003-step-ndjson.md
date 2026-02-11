# 2026-02-11 CLI-003 step NDJSON logs

- Step logs now emit a stable NDJSON schema per step with `action`, `outcome`, and `timings` fields.
- Added `StepOutcomeLog` in memory records to capture `done`, `validation_failed`, `apply_failed`, `no_progress`, and `progress` outcomes.
- CLI output uses the recorded outcome so downstream parsers do not need to infer it from validation/result details.
