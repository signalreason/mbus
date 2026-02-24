## Escalation transitions telemetry

- Step records now carry router telemetry: ladder index, router counters, trigger counters, and transition events.
- Transitions are collected once per step from the router and reused for both tracing logs and step JSON logs.
- Run summary aggregates transition counts by reason and includes final router state (model, effort, tier, ladder index).
