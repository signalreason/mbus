# 2026-02-24 LLM-006 multimodal request golden ordering + context struct

- Clippy flags `too_many_arguments` on LLM request builders; fix by using `LlmContext` to pass request inputs as a single struct.
- Multimodal golden prompt embeds schema JSON using `serde_json::to_string`, which sorts object keys; goldens must match the sorted key order.
- Updated the multimodal payload fixture to keep schema ordering stable with the serialized output.
