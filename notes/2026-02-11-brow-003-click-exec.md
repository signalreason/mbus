# 2026-02-11 BROW-003 click action execution

- Click execution now resolves element ids against the latest observation map and fails when the id is missing.
- CDP node resolution / box model lookups map detached or stale node errors to `stale_element` so callers get structured failures.
- This keeps action execution aligned with snapshot ids and prevents accidental clicks on non-observed nodes.
