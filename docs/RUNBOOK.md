# Exom Always-On Halls Runbook

## How to Host a Hall

1. Create or select a Hall from the left panel
2. If you have Agent role or higher, hosting starts automatically
3. Status line shows: `Connected (Host) - <ip>:<port>`
4. Your invite URL appears in the bottom right panel

## How to Invite Others

1. While hosting, look at the bottom right panel
2. Click the "Copy" button next to the invite URL
3. Share the copied URL with others (e.g., `exom://192.168.1.5:7331/<hall_id>/<token>`)
4. Others paste this URL in the "Join with Invite" field

## Status Line Meanings

| Status | Meaning |
|--------|---------|
| `Offline (local only)` | Not connected to any network |
| `Connecting...` | Attempting to connect |
| `Connected (Host) - <ip>:<port>` | You are hosting the hall |
| `Connected (Client) - <ip>:<port>` | Connected to another host |
| `Reconnecting... (retry N in Xs)` | Connection lost, auto-retry in progress |
| `Election in progress...` | Host died, choosing new host |
| `Copy failed` | Clipboard copy failed (shown for 3 seconds) |

## How to Recover if Stuck Reconnecting

1. Wait for the backoff cycle (1s, 2s, 5s, 10s, max 30s)
2. If the host is permanently gone, election will trigger
3. If election succeeds, you'll either become host or reconnect to new host
4. If no one can host (all Fellows), status changes to `Offline`

Manual recovery:
- Close and reopen the app to clear reconnect state
- The app will auto-reconnect to the last hall on startup

## How to Test Failover

### Basic Flow Test

1. **User A**: Create hall, start hosting
2. **User B**: Join via invite URL
3. **Both**: Send some chat messages
4. **Verify**: Messages appear on both sides

### Host Crash Test

1. Complete "Basic Flow Test"
2. **User A** (host): Force-close the app (Ctrl+C or kill)
3. **User B**: Watch status change to `Election in progress...`
4. **Wait**: Within 6 seconds, User B should become host (if Agent+)
5. **User B**: Status shows `Connected (Host) - <ip>:<port>`

### Reconnect Test

1. Complete "Host Crash Test"
2. **User A**: Restart the app
3. **User A**: App auto-reconnects to last hall
4. **User A**: Status shows `Reconnecting...`, then `Connected (Client)`
5. **Verify**: Previous messages appear (synced via SyncBatch)

### Message Ordering Test

1. Have multiple users connected
2. Send 50+ messages rapidly from different users
3. Disconnect host mid-stream
4. Reconnect after election completes
5. **Verify**: No duplicate messages, order is consistent

## Troubleshooting

### "No one can host - session ended"
All remaining users are Fellows (cannot host). Someone with Agent role or higher needs to create a new session.

### Messages appear out of order
Check if sequence numbers are being assigned. Local-only messages (sequence=NULL) sort after network messages.

### Copy button doesn't work
On Wayland, ensure `wl-copy` is installed. If both arboard and wl-copy fail, status briefly shows "Copy failed".

### Stuck in "Election in progress"
Election requires at least one user with Agent role or higher. If all users are Fellows, election fails and goes offline.
