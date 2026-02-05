# 2026-02-05 TYPES-001 core types + schema

- Added core serde types: `Observation`, `ElementRef`, `Action`, `StepResult`, `StepError`.
- `Action` is an internally tagged enum (`type`) with variants: click, type, select, scroll, wait, navigate, back, extract, done.
- `ActionSchema` uses `schemars` to generate JSON Schema and `jsonschema` to validate payloads, returning `SchemaViolation` entries.
- Extract currently requires a `query` string and optional `id`; bounds checks remain for later validator.
