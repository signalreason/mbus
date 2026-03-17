# 2026-03-17 RUNTIME-001/RUNTIME-002 priority shift

- The project goal is still a packaged real-model `mbus challenge` proof against the default 12-task obstacle suite.
- Browser runtime stability is now an explicit prerequisite, not an implicit assumption, because browser-backed validation is currently blocked by `cdp_launch_failed` before task execution begins in this environment.
- A lightweight browser startup preflight check is also now part of the active backlog so operators can fail fast before expensive `bench` or `challenge` runs.
- Backlog priority order is now: stable Chromium/CDP runtime, browser preflight, real-model proof package, then gate-hardening review.
