# CLI run task + max-steps flags

- `mbus run` args live in `src/main.rs` under `RunArgs`.
- `--task` and `--task-file` are now mutually exclusive and one is required (clap-level validation).
- `--plan` and `--plan-file` are mutually exclusive (clap-level validation).
- `--max-steps` validates as `usize` with a minimum of 1.
