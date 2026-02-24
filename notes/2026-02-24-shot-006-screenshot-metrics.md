# 2026-02-24 SHOT-006 screenshot metrics + summary

- Added telemetry counters for screenshot captures (total captures, bytes, duration) plus failure and persist-failure totals.
- CDP screenshot capture records duration/bytes and increments failure counters when capture errors or timeouts occur.
- Run summary now emits aggregate screenshot stats under `summary.screenshots`.
