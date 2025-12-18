# Contributing to Exom

## Development Setup

### Prerequisites

- Rust 1.75 or later
- System dependencies for Slint:
  - Linux: `libfontconfig1-dev libfreetype6-dev`
  - macOS: Xcode command line tools
  - Windows: Visual Studio Build Tools

### Building

```bash
cargo build --workspace
```

### Running

```bash
cargo run -p exom-app
```

## Code Quality

### Before Submitting

Run these checks locally before opening a PR:

```bash
# Format code
cargo fmt --all

# Lint
cargo clippy --workspace --all-targets --all-features

# Run tests
cargo test --workspace
```

All checks must pass. CI will reject PRs with failing checks.

### Code Style

- Use `rustfmt` defaults
- Eliminate all Clippy warnings
- No panics in library code - use `Result` types
- Add `#[instrument]` to public functions for tracing

### Error Handling

- Return `Result<T, Error>` instead of panicking
- Use the `Error` type from `exom_core::error`
- Propagate errors with `?` operator
- Log errors at the boundary where they are handled

### Testing

- Add tests for new functionality
- Tests go in `#[cfg(test)]` modules or `/tests` directories
- Use `tempfile` for tests that need filesystem access

## Project Structure

```
crates/
  core/         # Data models, permissions, hosting, storage
    src/
      chest/    # Hall Chest file management
      hosting/  # Host election logic
      models/   # Data structures
      permissions/  # Role-based access
      storage/  # SQLite persistence
  app/          # Slint UI application
    src/
      viewmodel/  # UI bindings
    ui/           # Slint markup files
```

## Pull Request Process

1. Fork the repository
2. Create a feature branch from `main`
3. Make changes with clear, focused commits
4. Ensure all checks pass locally
5. Open a PR with a description of changes
6. Address review feedback

### Commit Messages

Write clear commit messages that explain what and why:

```
Add permission check for message deletion

Messages can now only be deleted by the sender or users with
HallModerator role or higher. This prevents accidental deletion
by other members.
```

## Architecture Guidelines

### Permissions

- Enforce permissions in the core layer, not just UI
- Use `HallRole::can_*()` methods for permission checks
- Document required permissions in function comments

### Storage

- Use parameterized queries (no string interpolation for SQL)
- Return `Result` from all database operations
- Use `#[instrument]` for tracing database calls

### UI

- Keep view models thin - delegate to core
- Handle errors gracefully - show user-friendly messages
- Avoid blocking the UI thread

## Questions

If you have questions about contributing, open an issue for discussion.
