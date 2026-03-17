# 2026-03-17 PKG-001 challenge package bundles

- Added `mbus package` to turn an existing `challenge` report into a portable bundle plus zip archive.
- Packaging trusts `BenchReport.results[*].output_artifacts` as the source of truth for copied files and revalidates on-disk SHA-256 when the report includes one.
- Bundled artifact paths preserve the `.ralph/runs/...` tail under `artifacts/<task_id>/...` so screenshots and traces stay unique and readable across tasks.
- `manifest.json` inventories packaged payload files relative to the bundle root, but excludes `manifest.json` itself to avoid self-referential checksum recursion.
