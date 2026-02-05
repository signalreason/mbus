# OBS-001 Observation builder

- Element ids are stable across snapshots using a signature built from role,
  node name, accessible name, and key attributes (id/name/type/href/etc).
- Stable ids are formatted as `el_<hash>_<occurrence>` where `<hash>` is a
  deterministic FNV-1a 64-bit hex digest of the signature, and `<occurrence>`
  disambiguates duplicates by DOM order.
- CDP snapshot now returns a map from stable id to backend node id; the browser
  stores the latest map and action execution resolves ids through it first.
- Observation `state_hash` is computed from URL, title, and the top elements to
  detect navigation/progress reliably.
