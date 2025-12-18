# Exom Architecture

## Overview

Exom is a Hall-based collaboration platform built with Rust and Slint. This document describes the core architecture and design decisions.

## Crate Structure

```
exom/
├── crates/
│   ├── core/           # Core library
│   │   ├── models/     # Data models
│   │   ├── permissions/# Permission system
│   │   ├── hosting/    # Host election
│   │   ├── storage/    # SQLite persistence
│   │   └── chest/      # Local file management
│   └── app/            # Application
│       ├── ui/         # Slint UI files
│       └── viewmodel/  # UI bindings
```

## Core Concepts

### Hall

A Hall is the primary workspace unit. It contains:
- Members with assigned roles
- Chat messages
- Hosting state
- Optional Parlor module (future)

### Roles

Five-tier role hierarchy with ascending privileges:

| Role | Level | Description |
|------|-------|-------------|
| Hall Builder | 5 | Owner with full control |
| Hall Prefect | 4 | Admin with most permissions |
| Hall Moderator | 3 | Can manage members and messages |
| Hall Agent | 2 | Standard participant |
| Hall Fellow | 1 | Guest with limited access |

### Hosting

Hosting determines which member coordinates Hall activities.

**Priority**: Builder > Prefect > Moderator > Agent > Fellow

**Rules**:
- First eligible member entering an empty Hall becomes host
- Higher-role members joining are prompted to take over
- Host leaving triggers cascade election to next highest role
- Election epoch prevents split-host scenarios

### Hall Chest

Local folder storage for Hall files.

**Structure**:
```
~/.local/share/exom/chests/{hall-id}/
├── shared/      # Shared files
├── personal/    # User-specific files
└── downloads/   # Downloaded content
```

**Access**: Agent+ roles only. Fellows have no access.

**Status**: Local only. Sync is designed but not implemented.

## Permission Matrix

| Action | Builder | Prefect | Moderator | Agent | Fellow |
|--------|---------|---------|-----------|-------|--------|
| Delete Hall | Y | N | N | N | N |
| Edit Settings | Y | Y | N | N | N |
| Transfer Ownership | Y | N | N | N | N |
| Invite Members | Y | Y | Y | N | N |
| Kick Members | Y | Y | Y | N | N |
| Ban Members | Y | Y | N | N | N |
| Change Roles | Y | Y | N | N | N |
| Send Messages | Y | Y | Y | Y | Y |
| Delete Others' Messages | Y | Y | Y | N | N |
| Become Host | Y | Y | Y | Y | N |
| View Chest | Y | Y | Y | Y | N |
| Write Chest | Y | Y | Y | Y | N |
| Activate Parlor | Y | Y | N | N | N |

## Data Storage

SQLite database with the following tables:
- `users`: User accounts with password hashes
- `sessions`: Active login sessions
- `halls`: Hall metadata and hosting state
- `memberships`: User-Hall relationships with roles
- `messages`: Chat messages
- `invites`: Invitation tokens

## UI Architecture

Three-panel layout:

```
┌─────────────┬───────────────────────┬─────────────┐
│  Left       │  Center               │  Right      │
│  Panel      │  Panel                │  Panel      │
│             │                       │             │
│  Halls      │  Chat                 │  Members    │
│  List       │  Messages             │  List       │
│             │                       │             │
│  Create     │  Compose              │  Actions    │
│  Join       │  Bar                  │  Chest      │
└─────────────┴───────────────────────┴─────────────┘
```

**Theme**: Professional dark with slate blue accents.

## Future: Hall Parlors

Parlors are plugin modules that extend Hall functionality.

Planned examples:
- Education: Shared whiteboards, quizzes
- Watch-together: Synchronized video playback
- Code-together: Collaborative editing

**Interface** (defined but not implemented):
```rust
pub trait ParlorModule: Send + Sync {
    fn parlor_type_id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn on_activate(&mut self, hall_id: Uuid);
    fn on_deactivate(&mut self, hall_id: Uuid);
}
```

## Future: Networking

The architecture supports future networking:
- Hall state synchronization
- Real-time message delivery
- Host election coordination
- Chest file synchronization

Currently all operations are local.
