# 2026-02-11 OBSERV-002 LLM metrics

## Summary
- LLM latency emits a `llm_duration_ms` metric event on every LLM call.
- LLM failures increment `llm_failures_total` with `error_code` tag and are tracked per code in telemetry.

## Implementation detail
- `telemetry::inc_llm_failure(code)` records total + per-code counts and logs a structured metric event.
- `telemetry::record_llm_duration()` logs a structured metric event with the duration in milliseconds.
- OpenAI adapter now calls `inc_llm_failure(err.code)` so failures are tagged consistently.
