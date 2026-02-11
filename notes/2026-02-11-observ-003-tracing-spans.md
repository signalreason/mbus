# 2026-02-11 OBSERV-003 tracing spans for step stages

- Added per-step stage spans in `AgentLoop::run` for snapshot, llm, validation, and apply.
- Canonical names: `step.snapshot`, `step.llm`, `step.validation`, `step.apply`.
- Each span includes `step_index` and `tier` (model tier); initial snapshot uses `step_index = 0`.
- LLM adapter still emits its own `llm_call` span with model name; step-level span wraps it.
