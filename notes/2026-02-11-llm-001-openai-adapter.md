# 2026-02-11 LLM-001 OpenAI adapter

## Summary
- OpenAI-compatible LLM client already handles prompt construction, schema validation, and optional JSON repair.
- Added explicit error typing for request timeouts and transport failures so agent loop can distinguish them from generic HTTP errors.

## Implementation detail
- `src/llm/openai.rs` maps `reqwest::Error` to `LlmError` codes:
  - `timeout` for `err.is_timeout()`
  - `transport_error` for `err.is_connect()`
  - `http_error` for remaining cases

## Follow-ups
- If we need finer-grained HTTP failure buckets (e.g., DNS vs TLS vs body decode), extend the mapping once we confirm `reqwest` exposes the signal.
