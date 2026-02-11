# 2026-02-11 TYPES-001 Observation fields

- Observation now treats `state_hash` as a required string (no optional wrapper).
- `focused` stays optional but always serializes (null when unknown) to keep the field stable.
- Added field-level doc comments in `src/types.rs` to clarify semantics.
- Updated tests and helpers to reflect the required `state_hash`.
