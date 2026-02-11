# 2026-02-11 VALID-005 navigate URL policy parsing

- Split navigate URL parsing from policy evaluation for clearer errors.
- Added `invalid_url` errors for malformed URLs before scheme checks.
- Policy now checks parsed scheme (`http`/`https` unless `allow_insecure`).
