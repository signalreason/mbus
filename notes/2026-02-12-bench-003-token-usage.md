# Bench token usage capture (M3-BENCH-003)

- Added a `TokenUsage` core type plus `LlmResponse` so providers can return action + usage at the call boundary.
- OpenAI client now parses `usage` from chat completions and propagates it into step records.
- Step records include optional `llm_usage`, enabling per-task aggregation without touching action execution.
- Benchmark task results now report per-task prompt/completion/total token counts with explicit null + error strings when usage is missing.
- Scripted/stub modes surface a clear `usage_unavailable_for_*` error instead of fabricating zeros.
