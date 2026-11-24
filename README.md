# ztik

ztik is an unofficial desktop client for TikTok, built with Tauri. It opens TikTok in a desktop window and adds a system tray, mini-player mode, and media shortcuts.

## Features

- Browse TikTok in a dedicated desktop window
- Hide to the system tray when closing the window, then reopen from the tray
- Switch to an always-on-top mini-player
- Control playback, move to the next video, and repost with global shortcuts
- Pause media before hiding the app to the tray

## Keyboard shortcuts

| Shortcut | Action |
| --- | --- |
| `Ctrl+Alt+Shift+Space` | Play or pause the active video/audio |
| `Ctrl+Alt+Shift+Right` | Move to the next video |
| `Ctrl+Alt+Shift+R` | Repost the current video |

Global shortcut support depends on the desktop environment. Linux Wayland sessions may not allow applications to register global shortcuts.

## Downloads

Download the latest version from [GitHub Releases](https://github.com/hshhs910/ztik/releases):

- **Windows Portable:** `ztik-windows-x64-portable.exe` (run directly)
- **Windows Installer:** the NSIS `*-setup.exe` installer
- **Linux:** the `.AppImage` or `.deb` package

The Windows portable executable needs the Microsoft Edge WebView2 Runtime if it is not already installed.

## Build from source

Install the stable Rust toolchain and the Tauri v2 system prerequisites for your platform. On Ubuntu, install `libwebkit2gtk-4.1-dev`, `build-essential`, `curl`, `wget`, `file`, `libxdo-dev`, `libssl-dev`, `libayatana-appindicator3-dev`, and `librsvg2-dev`.

Install the Tauri CLI and build:

```sh
cargo install tauri-cli --version "^2" --locked
cargo tauri build
```

To build a specific Linux package, use `cargo tauri build --bundles deb,appimage`. On Windows, use `cargo tauri build --bundles nsis` to create the installer. Build output is written under `target/release`.

## License

ztik is distributed under the MIT License. See [LICENSE](LICENSE).

ztik is an unofficial client application and is not affiliated with, endorsed by, or sponsored by TikTok or ByteDance.
