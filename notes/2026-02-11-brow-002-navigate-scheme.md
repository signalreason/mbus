# BROW-002 navigate scheme validation

- Navigate validation trims URLs and treats scheme case-insensitively.
- Default policy only allows `http` and `https`; other schemes yield `insecure_url` validation errors.
- `allow_insecure = true` bypasses scheme restriction.
- Tests cover http/https plus uppercase scheme acceptance.
