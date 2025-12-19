# Exom Packaging Guide

This document covers building and running Exom on different Linux distributions using Nix and Flatpak.

---

## Quick Start

### Option 1: Nix (Recommended for NixOS/distro-agnostic)

```bash
# Run directly (downloads and builds)
nix run github:Onyx-Digital-Dev/exom

# Or from local checkout
nix run .#exom

# Development shell with all dependencies
nix develop
cargo run -p exom-app
```

### Option 2: Flatpak (Recommended for Ubuntu, Fedora, etc.)

```bash
# Build and install
flatpak-builder --user --install --force-clean build-dir io.exom.Exom.yml

# Run
flatpak run io.exom.Exom
```

---

## Nix Packaging

### Prerequisites

- Nix with flakes enabled
- For NixOS: flakes are usually enabled
- For other distros: `nix.conf` needs `experimental-features = nix-command flakes`

### Available Outputs

| Output | Command | Description |
|--------|---------|-------------|
| `packages.exom-app` | `nix build .#exom-app` | Build the binary |
| `apps.exom` | `nix run .#exom` | Run the application |
| `devShells.default` | `nix develop` | Development environment |

### Build Commands

```bash
# Build the package
nix build .#exom-app

# The binary will be at ./result/bin/exom-app
./result/bin/exom-app

# Or run directly
nix run .#exom

# Enter development shell
nix develop
cargo build -p exom-app
cargo test
```

### Cross-Platform Notes

The flake is configured for `eachDefaultSystem` which includes:
- `x86_64-linux`
- `aarch64-linux`
- `x86_64-darwin` (macOS - untested)
- `aarch64-darwin` (Apple Silicon - untested)

Linux builds include runtime library wrapping for:
- Slint GUI (fontconfig, freetype, OpenGL, Vulkan)
- X11 and Wayland support
- Clipboard (arboard)

### Known Limitations (Nix)

1. **First build is slow** - Downloads Rust toolchain and compiles all dependencies
2. **macOS untested** - Build inputs are Linux-specific, may need adjustment
3. **GPU drivers** - May need `nixGL` wrapper on non-NixOS systems for OpenGL

### Troubleshooting (Nix)

**"error: experimental Nix feature 'flakes' is disabled"**
```bash
# Add to ~/.config/nix/nix.conf or /etc/nix/nix.conf:
experimental-features = nix-command flakes
```

**OpenGL/rendering issues on non-NixOS:**
```bash
# Install nixGL
nix-env -iA nixpkgs.nixgl.nixGLDefault

# Run with nixGL wrapper
nixGL nix run .#exom
```

**"cannot find -lxkbcommon" or similar:**
```bash
# The flake should handle this, but if not:
nix develop  # Enter dev shell with all deps
cargo build -p exom-app
```

---

## Flatpak Packaging

### Prerequisites

- `flatpak` and `flatpak-builder` installed
- Freedesktop SDK: `flatpak install flathub org.freedesktop.Platform//24.08 org.freedesktop.Sdk//24.08`
- Rust extension: `flatpak install flathub org.freedesktop.Sdk.Extension.rust-stable//24.08`

### Build Commands

```bash
# Install SDK and runtime (first time)
flatpak remote-add --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
flatpak install flathub org.freedesktop.Platform//24.08
flatpak install flathub org.freedesktop.Sdk//24.08
flatpak install flathub org.freedesktop.Sdk.Extension.rust-stable//24.08

# Build and install (user-level)
flatpak-builder --user --install --force-clean build-dir io.exom.Exom.yml

# Run
flatpak run io.exom.Exom

# Run with debug output
flatpak run --env=RUST_LOG=debug io.exom.Exom
```

### Sandbox Permissions

The Flatpak manifest includes these permissions:

| Permission | Purpose |
|------------|---------|
| `--socket=wayland` | Wayland display |
| `--socket=fallback-x11` | X11 fallback |
| `--device=dri` | GPU/OpenGL access |
| `--share=network` | LAN networking |
| `--socket=x11` | Clipboard (X11) |
| `--filesystem=xdg-documents:create` | Hall Chest storage |
| `--filesystem=~/.local/share/exom:create` | App data directory |
| `--filesystem=/usr/share/fonts:ro` | System fonts |

### Files Included

| File | Purpose |
|------|---------|
| `io.exom.Exom.yml` | Flatpak manifest |
| `io.exom.Exom.desktop` | Desktop entry |
| `io.exom.Exom.metainfo.xml` | AppStream metadata |
| `io.exom.Exom.svg` | Application icon |

### Known Limitations (Flatpak)

1. **First build is slow** - Compiles Rust from scratch in sandbox
2. **Offline build not supported** - Cargo needs network access during build
3. **Icon is placeholder** - Replace `io.exom.Exom.svg` with actual icon
4. **No auto-updates** - Not published to Flathub yet

### Troubleshooting (Flatpak)

**"No remote refs found for 'org.freedesktop.Sdk.Extension.rust-stable'"**
```bash
# Make sure flathub is added
flatpak remote-add --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
flatpak update
flatpak install flathub org.freedesktop.Sdk.Extension.rust-stable//24.08
```

**Build fails with cargo network error:**
```bash
# Flatpak-builder needs network access for cargo
flatpak-builder --disable-download=false ...
```

**"Permission denied" accessing files:**
```bash
# Grant additional filesystem access
flatpak run --filesystem=home io.exom.Exom
```

**Clipboard not working:**
```bash
# X11 clipboard requires x11 socket
flatpak run --socket=x11 io.exom.Exom
```

---

## Native Build (Reference)

If you just want to build natively without packaging:

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install system dependencies (Ubuntu/Debian)
sudo apt install pkg-config libfontconfig1-dev libfreetype6-dev \
    libxkbcommon-dev libwayland-dev libx11-dev libxcursor-dev \
    libxi-dev libxrandr-dev libxcb1-dev libgl1-mesa-dev \
    libxfixes-dev libxext-dev

# Install system dependencies (Fedora)
sudo dnf install pkg-config fontconfig-devel freetype-devel \
    libxkbcommon-devel wayland-devel libX11-devel libXcursor-devel \
    libXi-devel libXrandr-devel libxcb-devel mesa-libGL-devel \
    libXfixes-devel libXext-devel

# Build
cargo build --release -p exom-app

# Run
./target/release/exom-app
```

---

## Testing on Multiple Distros

### Recommended Test Matrix

| Distro | Method | Notes |
|--------|--------|-------|
| NixOS | `nix run` | Native, best tested |
| Ubuntu 24.04 | Flatpak | Most common desktop |
| Fedora 40 | Flatpak | GNOME reference |
| Arch Linux | Native or Nix | Rolling release |

### Quick VM Test (with distrobox or toolbox)

```bash
# Ubuntu container
distrobox create -i ubuntu:24.04 -n exom-ubuntu
distrobox enter exom-ubuntu
# ... install flatpak, build, run

# Fedora container
distrobox create -i fedora:40 -n exom-fedora
distrobox enter exom-fedora
# ... install flatpak, build, run
```

---

## Data Locations

| Data | Native Path | Flatpak Path |
|------|-------------|--------------|
| Database | `~/.local/share/exom/` | `~/.var/app/io.exom.Exom/data/exom/` |
| Hall Chest | `~/Documents/ExomChest/` | `~/Documents/ExomChest/` |
| Config | `~/.config/exom/` | `~/.var/app/io.exom.Exom/config/exom/` |

---

## Future Work

- [ ] Publish to Flathub
- [ ] Create proper application icon
- [ ] Add AppImage packaging
- [ ] Test macOS builds via Nix
- [ ] Add systemd user service for background sync
