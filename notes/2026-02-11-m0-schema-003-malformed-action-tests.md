# M0-SCHEMA-003 malformed action payload tests

- Added coverage for malformed action payloads in LLM parsing paths.
- `OpenAiClient::parse_strict` emits `invalid_json` for non-JSON and `schema_violation` for missing fields, wrong types, and unknown actions.
- `ScriptedLlm::parse_actions` wraps malformed payloads with `invalid_actions`.
