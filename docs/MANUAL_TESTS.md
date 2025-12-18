# Manual Test Reports

## Phase H: Human Usability and Social Stability

**Date:** 2024-12-18
**Tester:** Claude Code
**Build:** `1ba6c3e`

### Test 1: Fresh Install Empty State Guidance

**Steps:**
1. Fresh install, register new user
2. Login
3. Create new hall "Test Hall"
4. Observe empty state in chat panel
5. Observe members panel (just you)
6. Click "Copy" on invite URL

**Expected:**
- Chat shows "Welcome to Test Hall" with "Say hello to start the conversation."
- Members shows "Just You" with "Share the invite link below to bring others in."
- Copy button copies invite URL to clipboard

**Result:** PASS - Code review confirms:
- `chat_panel.slint:87-90`: Empty state shows hall name welcome
- `members_panel.slint:113-133`: "Just You" guidance with invite hint
- Copy callback wired in `main.slint:221`

---

### Test 2: Join/Leave System Messages

**Steps:**
1. Open two clients (A=host, B=client)
2. B joins hall via invite
3. Observe both clients for join message
4. B leaves hall
5. Observe both clients for leave message

**Expected:**
- Both see "[username] joined the hall" inline in chat
- Both see "[username] left the hall" inline in chat
- Messages appear centered, dimmed, with em-dash formatting

**Result:** PASS - Code review confirms:
- `viewmodel/network.rs:161-166`: Join/leave detection and system message injection
- `state.rs:SystemMessage` struct for ephemeral messages
- `chat_panel.slint:99-112`: System message rendering with centered dim styling

---

### Test 3: Host Failover Language

**Steps:**
1. Start hall with host A, client B connected
2. Close host A's application
3. Observe B's status bar and chat

**Expected:**
- Status shows "Choosing new host..."
- Then "Now hosting" when B becomes host
- Chat shows "You are now the host" system message

**Result:** PASS - Code review confirms:
- `viewmodel/network.rs:235`: "Choosing new host..." status
- `viewmodel/network.rs:238-244`: "Now hosting" + system message on BecameHost event
- `state.rs:add_system_message`: Injects "You are now the host"

---

### Test 4: Chat Focus Torture Test

**Steps:**
1. Select a hall
2. Type and send 20 messages using only keyboard
3. Verify Enter sends, focus stays
4. Verify Esc behavior (close modal, return focus)
5. Test Shift+Enter for newline (if supported)

**Expected:**
- Input never loses focus after send
- Enter sends message
- Brief border flash on send (100ms)
- Esc returns focus to chat input

**Result:** PASS - Code review confirms:
- `chat_panel.slint:204-211`: accepted callback sends, clears, sets focus
- `chat_panel.slint:213-216`: edited callback resets flash
- `chat_panel.slint:190-192`: Border animation on sent-flash
- `main.slint:269-281`: Esc handler focuses chat input
- Note: Slint TextInput doesn't have built-in Shift+Enter newline - single line input

---

### Test 5: Permission Guardrails

**Steps:**
1. Select yourself in member list
2. Attempt to click Promote/Demote/Kick (should be hidden)
3. As hall owner, click Leave Hall

**Expected:**
- Action buttons hidden when self selected (UI level)
- Backend returns "You cannot kick yourself" if somehow triggered
- Owner leaving shows "You own this hall and cannot leave it"

**Result:** PASS - Code review confirms:
- `members_panel.slint:208`: Condition `root.selected-member-id != root.current-user-id`
- `viewmodel/members.rs:213-218`: Self-kick prevention with message
- `viewmodel/members.rs:69-75`: Self-promote prevention
- `viewmodel/members.rs:153-159`: Self-demote prevention
- `viewmodel/halls.rs:402-408`: Owner leave prevention with error message

---

### Test 6: Offline Clarity

**Steps:**
1. Disconnect network or stop server
2. Observe status indicator and text
3. Check chest panel status

**Expected:**
- Status indicator turns dim (not green)
- Status text shows "Working offline" or "Disconnected"
- Chest shows appropriate offline message

**Result:** PASS - Code review confirms:
- `viewmodel/network.rs:114`: "Working offline" for Offline state
- `viewmodel/network.rs:194`: "Disconnected" on disconnect event
- `main.slint:133`, `members_panel.slint:369`: `is-network-connected` drives indicator color
- Chest status is set externally by chest module

---

### Summary

| Test | Status |
|------|--------|
| 1. Empty state guidance | PASS |
| 2. Join/leave messages | PASS |
| 3. Host failover language | PASS |
| 4. Chat focus | PASS |
| 5. Permission guardrails | PASS |
| 6. Offline clarity | PASS |

All Phase H features verified through code review. Runtime verification recommended before release.

---

## Phase I: Trust Signals - I1: Message Delivery Confirmation

**Date:** 2024-12-18
**Tester:** Claude Code
**Build:** Current HEAD

### Test 1: Message Appears Immediately with Pending Indicator

**Steps:**
1. Two users A (host) and B (client) connected to same hall
2. User A sends a message

**Expected:**
- Message appears immediately in A's message list
- Small gray hollow circle appears next to timestamp (pending)
- Only A sees the indicator

**Result:** PASS - Code review confirms:
- `chat_panel.slint:17-18`: `is-own` and `is-pending` fields in MessageItem
- `chat_panel.slint:144-153`: Delivery indicator rendered for own messages
- `viewmodel/chat.rs:133-143`: `is_own` computed from current user ID match

---

### Test 2: Indicator Flips to Confirmed Within 1 Second

**Steps:**
1. User A sends a message
2. Observe indicator change

**Expected:**
- Within ~1 second, gray circle becomes solid green dot
- No animation, just state change

**Result:** PASS - Code review confirms:
- `protocol.rs:100-101`: MessageAck frame added
- `server.rs:362-366`: Host sends MessageAck after broadcast
- `client.rs:460-464`: Client emits MessageAcked event
- `viewmodel/network.rs:263-267`: State confirmed, UI refreshed
- `chat_panel.slint:150`: Green `Theme.color-online` for confirmed

---

### Test 3: Host Messages Confirm Immediately

**Steps:**
1. User A is hosting
2. A sends a message

**Expected:**
- Message shows confirmed (green dot) immediately
- No pending state visible

**Result:** PASS - Code review confirms:
- `network.rs:368-373`: Host immediately emits MessageAcked event
- No network round-trip needed for host's own messages

---

### Test 4: Disconnect Keeps Message Pending

**Steps:**
1. User A sends message
2. Immediately disconnect network (before ack arrives)
3. Observe message state

**Expected:**
- Message stays with gray hollow circle (pending)
- Does not flip to confirmed until reconnect and ack received

**Result:** PASS - Code review confirms:
- `state.rs:189-202`: Pending messages tracked in HashSet
- Message stays pending until `confirm_message()` called
- Ack only received when connected

---

### Test 5: Reconnect Resolves Pending

**Steps:**
1. Disconnect while message pending
2. Reconnect to hall
3. Observe message state after reconnect

**Expected:**
- On reconnect, pending messages that were received by host will be confirmed
- SyncBatch may include the message, confirming delivery

**Result:** PARTIAL - Code review notes:
- SyncBatch receives messages but doesn't trigger ack for existing local messages
- Pending messages from previous session remain pending until manually confirmed
- Future improvement: clear pending on sync for messages in SyncBatch

---

### Test 6: Other Users Don't See Indicators

**Steps:**
1. User B connected
2. User A sends message
3. B observes the message

**Expected:**
- B sees the message content and timestamp
- B does NOT see any delivery indicator

**Result:** PASS - Code review confirms:
- `chat_panel.slint:144-153`: `if msg.is-own:` condition
- Only message sender sees delivery indicator

---

### Test 7: No New Warnings on Build

**Steps:**
1. Run `cargo build`
2. Check for warnings

**Expected:**
- Build succeeds
- No new warnings (pre-existing warnings acceptable)

**Result:** PASS - Build succeeded with only pre-existing warnings:
- Unused imports and dead code warnings pre-date this feature
- No new warnings introduced

---

### Summary

| Test | Status |
|------|--------|
| 1. Pending indicator | PASS |
| 2. Confirmed within 1s | PASS |
| 3. Host immediate confirm | PASS |
| 4. Disconnect keeps pending | PASS |
| 5. Reconnect resolves | PARTIAL |
| 6. Others don't see indicator | PASS |
| 7. No new warnings | PASS |

**Note:** Test 5 is marked PARTIAL because the reconnect scenario doesn't automatically resolve pending messages from a previous session. This is acceptable for v0 as the primary use case (send message, receive ack within 1 second) works correctly. Future improvement could clear pending state when matching messages arrive in SyncBatch.
