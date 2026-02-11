# 2026-02-11 M1-HARNESS-001 static pages

- Added static harness pages under `harness/pages` for deterministic click/type/select coverage.
- Bench server now serves `harness/pages/bench/*.html` for `/bench/start` and `/bench/task-XX`.
- E2E harness server reads `harness/pages/actions.html` to keep tests aligned with the shared fixture.
