# OBS-004 AX node ID churn and repeat-action guard gap

- Root cause for repeated no-progress clicks in unchanged states:
  - `repeat_no_progress_action` checks exact action equality (`type` + `id`).
  - Observation element ids were derived from AX node ids.
  - AX node ids can churn across snapshots even when DOM and `state_hash` are unchanged.
- Effect:
  - Same semantic element appeared with a different `el_*` id on each snapshot.
  - Loop-level repeat-action validator did not trigger because action ids were different.
- Fix shipped:
  - Element signature now uses backend DOM node id (plus frame id/name/role context) instead of AX node id.
  - This makes `el_*` ids stable across unchanged snapshots, so exact-action repeat guard can block true repeats.
