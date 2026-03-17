# 2026-02-14 Site-005 no-progress threshold signature (step1?version=3)

Historical note. Superseded by `docs/live-eval-policy.md` and `docs/status.md`. This note describes an exploratory live-site run configuration, not the current proof workflow.

- Run terminated with `no_progress_termination` after exactly 3 unchanged hashes because `challenge.toml` sets `agent.max_no_progress_steps = 3`.
- Action application succeeded (`ok=true`, `error_code=none`) on each stalled step, so failure mode is strategy/no-op actions, not browser execution.
- Stalled steps proposed different click ids (`el_5a807...`, `el_ae970...`, `el_0f9f20...`) while `state_hash` and actionable signature stayed unchanged.
- Practical read: model is clicking real controls that do not mutate app state for this challenge step; without code entry/session extraction, progression cannot occur.
