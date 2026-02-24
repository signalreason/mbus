# 2026-02-24 LLM-010 reasoning effort request parameter

- Added `ReasoningEffort` enum (`low|medium|high`) and threaded it through `RouterConfig`, `LlmContext`, and `LlmRequest`.
- Router exposes the current effort via `Router::effort()`; the agent loop injects it into each LLM request.
- OpenAI chat payloads now include a top-level `reasoning_effort` field derived from the request.
- Config supports `router.reasoning_effort` (file/env/CLI) with `MBUS_ROUTER_REASONING_EFFORT` and `--router-reasoning-effort`.
- Tests cover router->context propagation, request serialization, and provider payload mapping.
