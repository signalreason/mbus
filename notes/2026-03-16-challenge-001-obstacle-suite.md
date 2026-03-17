# 2026-03-16 CHALLENGE-001 obstacle suite

- Added a new `mbus challenge` CLI path that reuses the existing agent execution flow and bench-style aggregate report shape, but forces `openai` mode and always persists screenshots.
- Challenge manifests now live under `harness/challenge/*.json` and use observable-only checks: `start_url`, `allowed_domains`, `max_steps`, `final_url_contains`, `final_visible_text_contains`, and optional `screenshot_artifact_required`.
- The local harness server now serves `harness/pages/challenge/*.html` in addition to the existing bench pages, so the obstacle suite stays reproducible and self-contained.
- `BenchReport` / `BenchTaskResult` were extended so both bench and challenge reports can carry `failure_buckets`, persisted `output_artifacts`, and optional final URL / visible text for debugging.
- Added a 12-task obstacle suite covering banners, modals, sticky consent, revealed CTAs, gated forms, interstitials, duplicate-intent buttons, accordions, tabs, scroll-load content, and checkbox-gated submission.
- Added integration coverage that runs the real `mbus` binary for both `bench --llm-mode scripted` and `challenge` against a local mock OpenAI server; the challenge assertion follows the gate semantics (`>=10/12`) instead of requiring every fixture to pass in CI.
