# 2026-02-11 TYPES-002 ElementRef flags shape

- Replaced string flag list with explicit `ElementFlags` fields on `ElementRef`.
- Flags now serialize as optional booleans (disabled/readonly/required/focused/editable/checked/selected/expanded/pressed/bbox_missing) to avoid ad hoc payloads.
- ElementRef continues to expose id/role/name/value/bbox with defaulted flags for backward-compatible deserialization.
