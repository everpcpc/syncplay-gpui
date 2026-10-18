<div align="center">

<img src="icon.svg" alt="Syncplay Icon" width="128" height="128">

# Syncplay

A modern, cross-platform Syncplay client built with Rust and [gpui-kit](https://github.com/longbridge/gpui-kit) (GPUI).

[![Latest release](https://img.shields.io/github/v/release/everpcpc/syncplay-gpui)](https://github.com/everpcpc/syncplay-gpui/releases/latest)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE)

**[macOS (Apple Silicon)](https://github.com/everpcpc/syncplay-gpui/releases/latest/download/syncplay_aarch64.dmg) · [macOS (Intel)](https://github.com/everpcpc/syncplay-gpui/releases/latest/download/syncplay_x64.dmg) · [Windows](https://github.com/everpcpc/syncplay-gpui/releases/latest/download/syncplay_x64-setup.exe) · [Linux](https://github.com/everpcpc/syncplay-gpui/releases/latest/download/syncplay_x86_64-unknown-linux-gnu.tar.gz)**

</div>

## Installation

Download the latest release for your platform — these links always point at the newest version:

| Platform | Download |
| --- | --- |
| macOS (Apple silicon) | [syncplay_aarch64.dmg](https://github.com/everpcpc/syncplay-gpui/releases/latest/download/syncplay_aarch64.dmg) |
| macOS (Intel) | [syncplay_x64.dmg](https://github.com/everpcpc/syncplay-gpui/releases/latest/download/syncplay_x64.dmg) |
| Windows | [syncplay_x64-setup.exe](https://github.com/everpcpc/syncplay-gpui/releases/latest/download/syncplay_x64-setup.exe) |
| Windows (portable) | [syncplay_x86_64-pc-windows-msvc.zip](https://github.com/everpcpc/syncplay-gpui/releases/latest/download/syncplay_x86_64-pc-windows-msvc.zip) |
| Linux | [syncplay_x86_64-unknown-linux-gnu.tar.gz](https://github.com/everpcpc/syncplay-gpui/releases/latest/download/syncplay_x86_64-unknown-linux-gnu.tar.gz) |

macOS builds are Developer ID signed and notarized. Windows binaries are unsigned, so SmartScreen may warn on first run; the installer is per-user and needs no admin rights.

The full asset list (including portable `tar.gz` / `zip` archives, which the in-app updater also consumes) is on the [releases page](https://github.com/everpcpc/syncplay-gpui/releases).

The app can update itself from the status bar (or automatically on startup) using the GitHub releases feed.

## Overview

Syncplay is a modern, cross-platform Syncplay client written entirely in Rust. It provides synchronized video playback across multiple users in real time. The UI is built with GPUI (the framework powering Zed) via the gpui-kit component library; the Syncplay protocol engine, player control, and synchronization logic live in a headless core crate.

### Features

- **Cross-platform**: Windows, macOS, and Linux
- **Native UI**: Rust + GPUI (gpui-kit components)
- **MPV Integration**: JSON IPC support (also VLC, MPlayer, MPC-HC/BE)
- **Real-time Sync**: Threshold-based sync + slowdown
- **Chat + Playlist**: Built-in chat and shared playlist

## Quick Start

### Prerequisites

- **Rust**: 1.85 or later
- **MPV**: Latest version with JSON IPC support

### Development

```bash
# Run in development mode
make run
```

### Building

```bash
# Build for production
make build

# Run tests
make test

# Lint
make lint
```

## Architecture

Cargo workspace with two crates:

- `crates/core` (`syncplay-core`): headless Syncplay engine — protocol codec and connection lifecycle, sync engine, shared playlist state machine, media index, player backends (mpv JSON IPC, VLC, MPlayer, MPC), configuration persistence.
- `crates/app` (`syncplay`): the GPUI desktop application. Core events reach the UI through an injected event sink; UI actions call back into the core's async command functions on a shared tokio runtime.

## Protocol Compatibility

This client is compatible with Syncplay protocol version 1.7.x and can connect to official Syncplay servers.

## License

Apache-2.0

## Acknowledgments

- Original Syncplay project: https://syncplay.pl/
- gpui-kit: https://github.com/longbridge/gpui-kit
- GPUI (Zed Industries): https://github.com/zed-industries/zed
- MPV player: https://mpv.io/
