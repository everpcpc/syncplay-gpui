# Syncplay

<div>
  <img src="icon.svg" alt="Syncplay Icon" width="128" height="128">
</div>

A modern, cross-platform Syncplay client built with Rust and [gpui-kit](https://github.com/longbridge/gpui-kit) (GPUI).

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

## Installation

Prebuilt packages are attached to each [release](https://github.com/everpcpc/syncplay-gpui/releases):

- **macOS**: signed and notarized `Syncplay` DMGs for Apple silicon (`aarch64`) and Intel (`x64`)
- **Windows**: a per-user NSIS installer (`x64-setup.exe`, no admin required)
- **Linux / portable**: raw binary archives (`tar.gz` / `zip`) that the in-app updater also consumes

The app can update itself from the status bar (or automatically on startup) using the GitHub releases feed.

## Notes

- macOS releases are Developer ID signed and notarized; Windows binaries are unsigned, so SmartScreen may warn on first run.

## License

Apache-2.0

## Acknowledgments

- Original Syncplay project: https://syncplay.pl/
- gpui-kit: https://github.com/longbridge/gpui-kit
- GPUI (Zed Industries): https://github.com/zed-industries/zed
- MPV player: https://mpv.io/
