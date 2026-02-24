# Router escalation ladder config

- Added config/CLI/env support for a router ladder as ordered `model:effort` entries.
- Supported inputs:
  - `router.ladder = ["gpt-5.1:medium", "gpt-5.2:high"]` in `mbus.toml`.
  - `--router-ladder gpt-5.1:medium --router-ladder gpt-5.2:high` on CLI.
  - `MBUS_ROUTER_LADDER=gpt-5.1:medium,gpt-5.2:high` in env.
- Normalization occurs during config load, mapping model names to tiers and validating transitions.
- Validation rejects empty ladders, model names not matching `llm.model_fast|mid|strong`, tier downgrades, and effort decreases within the same tier.
