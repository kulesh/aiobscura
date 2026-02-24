# aiobscura Edge Sensor Setup for Central CatSyphon

This document covers aiobscura-specific configuration when using aiobscura as an edge sensor that forwards data to a central CatSyphon server.

For full end-to-end central deployment (workspace setup, collector registration, launchd fleet pattern), use:

- <https://github.com/kulesh/catsyphon/blob/main/docs/collectors/edge-sensors-macos.md>

## 1. Install aiobscura on each edge Mac

```bash
brew install kulesh/tap/aiobscura
```

## 2. Configure collector credentials

Create `~/.config/aiobscura/config.toml`:

```toml
[collector]
enabled = true
server_url = "https://catsyphon.yourdomain.com"
collector_id = "REPLACE_WITH_COLLECTOR_ID"
api_key = "REPLACE_WITH_API_KEY"
batch_size = 20
flush_interval_secs = 5
timeout_secs = 30
max_retries = 3
```

`collector_id` and `api_key` come from `POST /collectors` on the CatSyphon server.

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
