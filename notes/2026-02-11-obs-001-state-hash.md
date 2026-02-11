# OBS-001 Deterministic state hash

- State hash now normalizes URL + title and hashes a stable, order-insensitive
  subset of actionable element signatures.
- Element signatures use role/name/value/flags (no element ids) to avoid
  backend-node churn influencing progress detection.
- Element signatures are sorted and the first 20 are used for the hash to keep
  the snapshot compact but deterministic.
- Added tests for order insensitivity and value-change hash updates.
