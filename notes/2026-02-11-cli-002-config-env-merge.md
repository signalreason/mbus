# 2026-02-11 CLI-002 config file + env merge

- Config path resolution now checks `--config`, `MBUS_CONFIG`, `./mbus.toml`, then `~/.mbus.toml`.
- Startup config logs redact URL userinfo to avoid leaking secrets in `base_url`/`cdp_url`.
- README config section updated to document lookup order and `MBUS_CDP_URL` override.
