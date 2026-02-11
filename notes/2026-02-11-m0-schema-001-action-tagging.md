## M0-SCHEMA-001 action tagging

- `Action` already uses an internally tagged enum with `type` + `snake_case` and `deny_unknown_fields` in `src/types.rs`.
- Added tests to ensure missing/unknown `type` fails deserialization.
