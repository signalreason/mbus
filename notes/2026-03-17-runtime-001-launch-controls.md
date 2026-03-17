# 2026-03-17 RUNTIME-001 launch controls + bootstrap wiring

- `chromiumoxide` 0.8 already exposes the launch hooks needed for runtime hardening: explicit executable path, `no_sandbox`, extra args, and launch timeout.
- `mbus` previously did not expose those controls through its own config or CLI, so all launch failures collapsed into a narrow default path.
- The existing `cdp_bootstrap` binary is a good manual startup validator once it accepts the same browser launch config as `mbus`; it does not need the broader `RUNTIME-002` UX to be useful now.
- Startup diagnostics are most actionable when they carry launch context directly in the `cdp_launch_failed` message: executable path, sandbox mode, launch timeout, user-data-dir path, and extra args.
