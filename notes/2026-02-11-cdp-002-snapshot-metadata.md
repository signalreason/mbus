# 2026-02-11 CDP-002 Snapshot metadata

- Snapshot already captures `url`, `title`, and `viewport` via CDP (`Page::url`, `Page::get_title`, `Page::layout_metrics`).
- Added an e2e test that asserts URL/title match the harness page and viewport dimensions are non-zero.
