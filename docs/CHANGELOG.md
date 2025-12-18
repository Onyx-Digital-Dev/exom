# Changelog

## Phase F: Always-On Halls

### F1: Persist Last Hall + Auto-Reconnect
- **Commit**: `b52b4a9`
- Store last connection per user in SQLite (last_connections table)
- Auto-reconnect on app launch with exponential backoff (1s, 2s, 5s, 10s, 30s cap)
- Reconnecting state in NetworkManager

### F2: Deterministic Host Election + Automatic Failover
- **Commit**: `b468286`
- Host heartbeat every 2 seconds
- Client detects host dead after 6 seconds of no heartbeat
- Deterministic election: highest role wins, tie-breaker by user_id (ascending)
- Winner starts server on port 7331 (increment if busy, up to +20)
- Epoch tracking to prevent stale reconnects

### F3: Message ID Dedupe + Host Sequence + Sync
- **Commit**: `8042a8f`
- Sequence field on NetMessage (host-assigned ordering)
- Server tracks message history (circular buffer, max 500)
- SyncSince/SyncBatch protocol for message sync on reconnect
- INSERT OR IGNORE for message deduplication by ID

### F4: Hall Settings + Copy Invite URL
- **Commit**: `fb14fc8`
- MembersPanel footer shows network status and invite URL
- Copy button copies invite URL to system clipboard (arboard)
- Network status indicator in right panel
