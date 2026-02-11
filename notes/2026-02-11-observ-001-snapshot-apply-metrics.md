# OBSERV-001 snapshot/apply metrics

- Made per-step snapshot/apply durations always emitted in step timings.
- For steps without apply/snapshot (done, validation failure, LLM error), durations are recorded as 0ms.
- Telemetry now records snapshot/apply duration metrics on every step boundary without adding labels.
