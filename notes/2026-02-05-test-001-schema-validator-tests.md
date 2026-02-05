# TEST-001 schema + validator tests

- Expanded `ActionSchema` tests to cover every action variant, including optional fields.
- Added invalid schema cases for missing required fields, unknown action types, and extra fields.
- Extended validator tests to cover click/type/select/scroll/wait/navigate/extract/back/done paths.
- Added explicit cases for missing URL, insecure URL, unknown ids, and in-bounds checks.
