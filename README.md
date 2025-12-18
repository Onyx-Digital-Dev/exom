# Exom

Hall-based collaboration platform built with Rust and Slint.

## Overview

Exom provides shared workspaces called "Halls" where members can collaborate with role-based permissions, chat, and local file storage (Hall Chest).

## Architecture

```
crates/
  core/     - Models, permissions, hosting logic, SQLite storage
  app/      - Slint UI and view models
```

### Core Concepts

- **Hall**: A workspace with members, roles, chat, and hosting state
- **Roles**: Five-tier permission system
  - Hall Builder (Owner) - Full control
  - Hall Prefect (Admin) - Management permissions
  - Hall Moderator - Chat moderation
  - Hall Agent (Member) - Standard participation
  - Hall Fellow (Guest) - Read-only access
- **Hosting**: Dynamic host election based on role priority
- **Hall Chest**: Local folder storage for Hall files (sync planned for future)

### Future: Hall Parlors

Parlors are plugin modules that extend Hall functionality. Examples include education tools, watch-together, and code-together features. The interface is defined but not implemented.

## Building

### Prerequisites

- Rust 1.75 or later
- System dependencies:
  - Linux: `sudo apt-get install libfontconfig1-dev libfreetype6-dev`
  - macOS: Xcode command line tools
  - Windows: Visual Studio Build Tools

### Build

```bash
cargo build --release
```

### Run

```bash
cargo run -p exom-app
```

### Test

```bash
cargo test --workspace
```

## Development

See [CONTRIBUTING.md](CONTRIBUTING.md) for development guidelines.

### Quick Checks

```bash
cargo fmt --all           # Format code
cargo clippy --workspace  # Lint
cargo test --workspace    # Run tests
```

## Technology

- Language: Rust
- UI: Slint
- Storage: SQLite (rusqlite)
- Password hashing: Argon2
- Logging: tracing

## Project Status

This is the core foundation. Network backend and Parlor plugins are planned for future phases.

## License

See LICENSE file.
