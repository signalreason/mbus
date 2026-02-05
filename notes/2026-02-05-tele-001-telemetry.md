# TELE-001 Telemetry

- Added `telemetry::init_tracing()` to enable JSON structured logs on stderr with env filtering.
- Added per-step tracing spans and step outcome logs in `AgentLoop::run` with redacted action summaries.
- Added lightweight metrics counters/durations in `telemetry` for steps, actions, LLM calls, and failures.
- Instrumented LLM clients to record call counts and durations; OpenAI client logs failure codes.
- Added a unit test to verify metrics counters increment.
- Note: CLI step logs still emit full actions (including typed text) and may need redaction.
