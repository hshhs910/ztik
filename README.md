# ztik // Lightweight TikTok Desktop Client

A fast, distraction-free **TikTok desktop client** for **Windows & Linux** built with **pure Rust and Tauri v2** (Zero-Frontend) — featuring **watermark-free media downloads**, a **frameless custom UI**, **deep WebView2 RAM & EcoQoS optimization**, a **9:16 Always-on-Top Mini-player**, global hotkeys, and strict upload blocking.

[![Release](https://img.shields.io/github/v/release/hshhs910/ztik?style=flat-square&label=Release&color=FE2C55)](https://github.com/hshhs910/ztik/releases/latest)
![Core](https://img.shields.io/badge/Core-Rust%20%2B%20Tauri%20v2-E44D26?style=flat-square)
![Frontend](https://img.shields.io/badge/Node.js-Zero-44CC11?style=flat-square)
![Platform](https://img.shields.io/badge/Platform-Windows%20%7C%20Linux-8A2BE2?style=flat-square)
![Tray RAM](https://img.shields.io/badge/Tray%20RAM-~19%20MB-00B4D8?style=flat-square)
[![License](https://img.shields.io/badge/License-MIT-007EC6?style=flat-square)](LICENSE)

> [!TIP]
> Click the window close button (`×`) to hide `ztik` to the System Tray and suspend WebView2 memory down to ~19 MB on Windows.

## Features

- **Distraction-free browsing:** Dedicated desktop window with persistent login sessions, hiding clutter like Upload, Get Coins, Get App, and Promote buttons, and automatically dismissing "Get Coins / Recharge" promotional popups
- **Watermark-free media downloader:** Download original-quality videos and multi-image Slideshow posts without the TikTok watermark directly into `Downloads/ztik/` via the titlebar button, tray menu, or global shortcut
- **Modern frameless UI & splash screen:** Custom borderless titlebar with Windows 11 DWM rounded corners, quick download button, redesigned window controls, and a black-background TikTok logo launch screen
- **Upload-free by design:** Strictly blocks `/upload` and TikTok Studio routes while keeping full viewing and social interactions (FYP, Repost, Comment, DM, Edit Profile)
- **Deep memory & performance optimization:** Dynamic Windows Efficiency Mode (EcoQoS) when unfocused, minimized, or hidden to the tray; Smart Idle working-set trimming and WebView2 `MemoryUsageTargetLevel::Low` + COM `TrySuspend`; event-driven offscreen video & feed card management; and a one-click **Free Memory (Dọn RAM ngay)** action in the System Tray
- **9:16 Mini-player:** Compact Always-on-Top window that strips away sidebars for effortless background watching
- **Global shortcuts:** Control playback, skip videos, repost, and download watermark-free media from anywhere on your desktop
- **Hardened security:** Zero remote IPC exposure (`withGlobalTauri: false`) and strict HTTPS domain whitelist for both main and OAuth popup windows

## Keyboard shortcuts

| Shortcut | Action |
| --- | --- |
| `Ctrl+Alt+Shift+Space` | Play or pause the active video/audio |
| `Ctrl+Alt+Shift+Right` | Move to the next video |
| `Ctrl+Alt+Shift+R` | Repost the current video |
| `Ctrl+Alt+Shift+D` | Download current video/photos without watermark |

> [!NOTE]
> Global shortcut support depends on the desktop environment. Linux Wayland sessions may restrict applications from registering global shortcuts.

## Downloads

Download the latest version from [GitHub Releases](https://github.com/hshhs910/ztik/releases):

- **Windows Portable:** `ztik-windows-x64-portable.exe` (run directly, no installation required)
- **Windows Installer:** `*-setup.exe` (NSIS installer)
- **Linux:** `.AppImage` (universal) or `.deb` (Ubuntu / Debian)
- **Integrity:** Verify downloads using the included `SHA256SUMS.txt`

The Windows portable executable requires the Microsoft Edge WebView2 Runtime (pre-installed on Windows 11 and most Windows 10 systems).

## Build from source

Install the stable Rust toolchain and the Tauri v2 system prerequisites for your platform. On Ubuntu/Debian, install:

```sh
sudo apt update && sudo apt install -y libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
```

Install the Tauri CLI and build:

```sh
cargo install tauri-cli --version "^2" --locked
cargo tauri build
```

To build a specific Linux package, use `cargo tauri build --bundles deb,appimage`. On Windows, use `cargo tauri build --bundles nsis` to create the installer. Build output is written under `target/release`.

## License

`ztik` is distributed under the MIT License. See [LICENSE](LICENSE).

`ztik` is an unofficial client application and is not affiliated with, endorsed by, or sponsored by TikTok or ByteDance.
