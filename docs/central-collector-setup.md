# aiobscura Edge Sensor Setup for Central CatSyphon

This document covers aiobscura-specific configuration when using aiobscura as an edge sensor that forwards data to a central CatSyphon server.

For full end-to-end central deployment (workspace setup, collector registration, launchd fleet pattern), use:

- <https://github.com/kulesh/catsyphon/blob/main/docs/collectors/edge-sensors-macos.md>

## 1. Install aiobscura on each edge Mac

```bash
brew install kulesh/tap/aiobscura
```

## 2. Register edge collector and auto-configure aiobscura

Run this once per edge machine:

```bash
aiobscura-collector register \
  --server-url "https://catsyphon.yourdomain.com" \
  --workspace-id "<WORKSPACE_UUID>"
```

Defaults:
- `collector_type` defaults to `aiobscura`
- `hostname` defaults to `uname -n` (override with `--hostname`)

The command calls `POST /collectors` and writes `~/.config/aiobscura/config.toml` with:
- `enabled = true`
- `server_url`
- `collector_id`
- `api_key`

If credentials already exist in config, rerun with `--force` to rotate/replace them.

## 3. Run sync and publish continuously

```bash
aiobscura-sync --watch --poll 5000
```

## 4. Validate edge publish state

```bash
aiobscura-collector status
aiobscura-collector sessions
```

If an edge loses network and later reconnects:

```bash
aiobscura-collector resume
```

## Notes

- aiobscura is local-first: it writes to local SQLite first, then publishes.
- Collector lifecycle is explicit: `session_start`, incremental events, and session completion are sent to CatSyphon.
