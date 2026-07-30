# cosmic-hot-corners

A native **Hot Corners** daemon for the [COSMIC™ Desktop](https://system76.com/cosmic) (Pop!_OS), built entirely with [libcosmic](https://github.com/pop-os/libcosmic) and [iced](https://github.com/iced-rs/iced).

## Overview

`cosmic-hot-corners` runs as a background daemon with no visible window. It places four invisible 10×10 px [wlr-layer-shell](https://wayland.app/protocols/wlr-layer-shell-unstable-v1) surfaces — one at each screen corner — using `Layer::Overlay`. When the pointer enters a corner and remains there longer than the configured delay, a configurable action is triggered.

The project is fully integrated with the COSMIC ecosystem: configuration is stored and watched via `cosmic-config`, and all Wayland interaction is handled through libcosmic's `iced_winit` + `cctk` (COSMIC Client Toolkit) backend.

## Architecture

```
main.rs
├── cosmic-hot-corners            // opens settings GUI (default)
└── cosmic-hot-corners --daemon   // runs the daemon (no visible window)

app.rs
├── init()         Loads config; waits for Wayland output events to create surfaces.
├── subscription() Listens to OutputEvent (Added/Removed) to manage surfaces per monitor,
│                  CursorMoved/CursorLeft for pointer detection, and watch_config() for
│                  live config reload without restarting the daemon.
├── update()
│   ├── OutputAdded   → creates 4 layer-shell surfaces (10×10 px, Overlay) for that output
│   ├── OutputRemoved → destroys the 4 surfaces for that output
│   ├── CursorMoved   → increments pending_generation, schedules tokio::time::sleep(delay_ms)
│   ├── TriggerCorner → fires action only if generation still matches
│   │                   (cancellation-safe — rapid movement never triggers)
│   └── ConfigUpdated → hot-reloads config; next trigger uses new settings
└── execute_action() Dispatches CornerAction variants via D-Bus or sh -c.

config.rs
└── Config (cosmic-config v1)
    ├── delay_ms: u64          // activation delay in milliseconds
    ├── top_left: CornerAction
    ├── top_right: CornerAction
    ├── bottom_left: CornerAction
    └── bottom_right: CornerAction

settings_app.rs
└── GUI settings window (libcosmic, follows COSMIC light/dark theme automatically)
    ├── Activation delay field
    └── 2×2 grid of corner cards, each with action dropdown + optional command input
```

## Corner Actions

| Variant | Behavior |
|---|---|
| `Disabled` | No-op |
| `ShowWorkspaces` | Opens the workspaces overview via D-Bus (`com.system76.CosmicWorkspaces`) |
| `OpenLauncher` | Opens the app launcher via D-Bus (`com.system76.CosmicLauncher`) |
| `RunCommand(String)` | Executes an arbitrary shell command via `sh -c` |

## Configuration

### GUI (recommended)

After installing, open **Hot Corners Settings** from the COSMIC app drawer, or right-click the daemon's entry in the launcher and choose **Configure Hot Corners**.

The settings window lets you assign an action to each corner and adjust the activation delay. Changes are saved instantly — no restart required.

### Manual (advanced)

Configuration is also stored as plain files managed by `cosmic-config`, under:

```
~/.config/cosmic/io.github.cosmic-hot-corners/v1/
```

Each field is a separate file. Example — enable workspace overview on the top-left corner with a 400 ms delay:

```sh
mkdir -p ~/.config/cosmic/io.github.cosmic-hot-corners/v1
echo '400' > ~/.config/cosmic/io.github.cosmic-hot-corners/v1/delay_ms
echo 'ShowWorkspaces' > ~/.config/cosmic/io.github.cosmic-hot-corners/v1/top_left
```

## Requirements

- COSMIC Desktop (Wayland compositor with `wlr-layer-shell` support)
- `libxkbcommon-dev`, `libwayland-dev`, `pkg-config`
- Rust toolchain (edition 2024)

## Installation

### AUR (Arch / CachyOS)

```sh
yay -S cosmic-corners-enhanced-git
systemctl --user enable --now io.github.cosmic-hot-corners.service
```

The package installs the COSMIC-only user service at
`/usr/lib/systemd/user/io.github.cosmic-hot-corners.service`; pacman removes it
again when the package is uninstalled. Enabling it is deliberately per-user, so
it starts only in your COSMIC session.

### 1. Install build dependencies

```sh
sudo apt install libxkbcommon-dev libwayland-dev pkg-config
```

### 2. Clone and install

```sh
git clone https://github.com/your-username/cosmic-hotcorners
cd cosmic-hotcorners
just build-release
sudo just install
just enable-service
```

### 3. Open the settings

Launch the settings GUI to configure each corner:

```sh
cosmic-hot-corners
```

Or open **Hot Corners Settings** from the COSMIC app drawer.

### 4. Service management

The installed user service starts the daemon only in COSMIC sessions:

```sh
just enable-service
```

To stop it and prevent future starts:

```sh
just disable-service
```

Configuration changes made in the settings GUI are applied instantly — the running daemon reloads config automatically without needing a restart.

To stop a running instance:

```sh
pkill -f cosmic-hot-corners
```

For distribution packaging:

```sh
just vendor
just build-vendored
just rootdir=debian/cosmic-hot-corners prefix=/usr install
```

## Flatpak

The project ships a Flatpak manifest at `io.github.cosmic-hot-corners.yml`.

### Build and test locally

**1. Install dependencies:**

```sh
sudo apt install flatpak flatpak-builder
flatpak remote-add --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak install flathub org.freedesktop.Platform//25.08 org.freedesktop.Sdk//25.08 \
    org.freedesktop.Sdk.Extension.rust-stable//25.08
```

**2. Generate cargo sources** (required once, and again after `Cargo.lock` changes):

```sh
pip install aiohttp toml
just flatpak-sources
```

**3. Build and install locally:**

```sh
just flatpak-build
```

**4. Test:**

```sh
just flatpak-run-settings   # open settings GUI
just flatpak-run            # run daemon
```

### Submit to Flathub

1. Create a GitHub repo named `io.github.cosmic-hot-corners` under the [flathub](https://github.com/flathub) org (via a PR to [flathub/flathub](https://github.com/flathub/flathub))
2. In that repo: copy `io.github.cosmic-hot-corners.yml` and `generated-sources.json`, adjusting the source from `type: dir` to a versioned archive or git tag
3. The Flathub CI builds and validates the manifest — the team reviews and merges

## Development

Install [rustup][rustup] and configure your editor with [rust-analyzer][rust-analyzer]. To reduce compile times, disable LTO in the release profile and use [mold][mold] + [sccache][sccache].

## License

[MPL-2.0](./LICENSE)

[just]: https://github.com/casey/just
[rustup]: https://rustup.rs/
[rust-analyzer]: https://rust-analyzer.github.io/
[mold]: https://github.com/rui314/mold
[sccache]: https://github.com/mozilla/sccache
