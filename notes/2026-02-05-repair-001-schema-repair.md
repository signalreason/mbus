# REPAIR-001 Schema Repair

- Implemented local repair for malformed LLM action payloads in `src/verify/repair.rs`.
- Heuristics: strip code fences, extract balanced JSON, unwrap `action`, take first array item,
  normalize `action_type`, map `option` -> `value`, coerce basic types, drop unknown fields.
- OpenAI client now attempts repair after strict parse failures and logs success/failure events.
- Telemetry adds `repair_attempts_total` and `repair_success_total` for repair success rate.

Notes:
- Repair is intentionally conservative; it only mutates when it can infer a safe mapping.
- Invalid required fields still fail schema validation and surface as `repair_failed`.
