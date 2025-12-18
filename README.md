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
- **Roles**: Hall Builder (Owner), Hall Prefect (Admin), Hall Moderator, Hall Agent (Member), Hall Fellow (Guest)
- **Hosting**: Dynamic host election based on role priority
- **Hall Chest**: Local folder storage for Hall files (sync planned for future)

### Future: Hall Parlors

Parlors are plugin modules that extend Hall functionality. Examples include education tools, watch-together, and code-together features. The interface is defined but not implemented.

## Building

```bash
cargo build --release
```

## Running

```bash
cargo run -p exom-app
```

## Technology

- Language: Rust
- UI: Slint
- Storage: SQLite
- Password hashing: Argon2

## Project Status

This is the core foundation. Network backend and Parlor plugins are planned for future phases.
