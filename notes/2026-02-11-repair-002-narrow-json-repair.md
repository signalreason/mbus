# REPAIR-002 Narrow JSON repair pass

- Expanded OpenAI parse flow to attempt repair on invalid JSON, action wrappers, and single-item arrays.
- Repair stays deterministic: only unwraps arrays when length == 1 and ignores multi-action arrays.
- Repair failures now preserve the original error code while appending repair context to the message.
