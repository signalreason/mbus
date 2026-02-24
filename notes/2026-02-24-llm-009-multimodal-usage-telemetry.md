# 2026-02-24 LLM-009 multimodal usage + payload telemetry

- Step logs now include `llm_payload_mode`, `image_context_sent`, and `llm_usage` when returned so NDJSON can distinguish text-only vs multimodal calls.
- Telemetry tracks per-mode LLM call counters (`llm_calls_text_total`, `llm_calls_multimodal_total`) alongside total calls.
- Payload mode is captured at request dispatch and propagated through `LlmResponse` into step records.
