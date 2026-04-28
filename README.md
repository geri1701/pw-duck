# pw-duck

`pw-duck` is a small Linux tray application that lowers non-voice audio while a configured remote voice stream is active.

The important detail: detection runs on the **incoming remote voice playback stream** of the selected application, not on the local microphone.

## How it works

- Select a voice source once, for example a Discord/WebRTC playback stream.
- While ducking is enabled, non-voice playback streams are routed through a virtual PipeWire collector sink.
- `pw-duck` measures the level of the configured remote voice source.
- When remote speech is detected, the collector path is reduced to the configured ducking volume.
- On stop, streams are restored best-effort. If a stream cannot safely be moved back, the virtual sink is intentionally left alive instead of destroying active browser/game audio.

## Requirements

Runtime expectations:

- PipeWire with WirePlumber or a compatible session manager
- PulseAudio compatibility (`pactl` must work against PipeWire/Pulse)
- `pw-link` from PipeWire
- a StatusNotifierItem/SNI tray host
- GTK4 for the optional graphical tuner window

The Nix package provides direct program dependencies and wrapper paths. PipeWire/WirePlumber and the tray host remain system/desktop services. The default Rust build intentionally excludes GTK; the GUI is enabled with the Cargo feature `gui`.

### Desktop support

- **KDE Plasma:** primary target; SNI is supported natively.
- **GNOME:** requires an AppIndicator/KStatusNotifierItem extension, for example “AppIndicator and KStatusNotifierItem Support”.
- Other SNI hosts may work, but click and menu behavior can differ.

## Run with Nix

From the project directory:

```bash
nix run .#
```

This starts the tray app. Explicit app outputs are also available:

```bash
nix run .#tray
nix run .#tune-gui
nix run .#tune
```

CLI commands:

```bash
nix run .# -- status
nix run .# -- sources
nix run .# -- select-source <sink-input-index>
nix run .# -- config-path
```

## First use

1. Start the voice application and join a call/channel so its playback stream is visible.
2. Start the tray:

   ```bash
   nix run .#
   ```

3. In the tray menu, choose `Controls:` → `Source: choose` and select the voice stream.
4. Enable `Ducking`.
5. Use `Tuner: open` if needed.

Alternatively via CLI:

```bash
nix run .# -- sources
nix run .# -- select-source <sink-input-index>
nix run .#
```

If a source is shown as `#546`, pass only the number in the shell command:

```bash
nix run .# -- select-source 546
```

## Tray menu

The menu intentionally separates information from controls:

```text
Info:
  Ducking: ON/OFF
  Details: ...
  Source: ...
  Tuning: ...

Controls:
  Ducking          # pure switch
  Tuner: open      # disabled in non-GUI builds
  Source: choose

Quit
```

The main visible tray state is intentionally binary:

- `Ducking OFF`
- `Ducking ON`

Internal states such as waiting, starting, neutral, ducked, or error are shown only in details.

## Tuner

The GTK window `tune-gui` exposes three values:

- **Sensitivity:** 0% disables VAD completely; 100% is very sensitive.
- **Ducking volume:** target volume for non-voice audio while remote speech is active, in 1% steps.
- **Hold:** time in milliseconds before ducking is released after speech ends.

Values are saved immediately and are picked up live by a running tray process. In builds without the Cargo feature `gui`, `pw-duck tune` remains available as a terminal tuner; the tray menu item for the GUI is disabled.

## Configuration

The configuration file is:

```text
~/.config/pw-duck/config.toml
```

Persisted values:

- `duck_percent`
- `vad_threshold`
- `hold_ms`
- the stable identity of the selected voice source

Runtime state is not persisted: `Ducking ON/OFF`, virtual sink names, currently routed streams, and VAD state live only in the running process.

## Development

This project needs native PipeWire/pkg-config libraries. The GTK GUI is optional and controlled by the Cargo feature `gui`.

Non-GUI paths build without GTK:

```bash
direnv exec . cargo check
direnv exec . cargo test
direnv exec . cargo run -- status
direnv exec . cargo run -- sources
```

GUI/tray development:

```bash
direnv exec . cargo run --features gui
direnv exec . cargo run --features gui -- tune-gui
direnv exec . cargo check --features gui
```

Or interactively:

```bash
nix develop
cargo run --features gui
```

Do not rely on plain `cargo` outside the dev shell if the shell says:

```text
Command 'cargo' not found; attempting execution with nix run...
```

That fetches only Cargo ad hoc, not the required PipeWire/pkg-config development environment. Typical failures are missing `.pc` files such as `pipewire-0.3.pc`; with the GUI feature also `glib-2.0.pc`, `gtk4.pc`, `cairo.pc`, and related files.

## Packaging

Build:

```bash
nix build .#
```

The package installs:

```text
bin/pw-duck
share/applications/pw-duck.desktop
share/icons/hicolor/.../apps/pw-duck.png
share/icons/hicolor/.../apps/pw-duck-symbolic.png
share/doc/pw-duck/README.md
share/doc/pw-duck/LICENSE
```

Flake app outputs:

```text
.#          pw-duck tray app
.#tray      pw-duck tray
.#tune-gui  pw-duck tune-gui
.#tune      pw-duck tune
```

The Nix package builds with the `gui` feature enabled, so the graphical tuner is included. The wrapper adds `pactl` and `pw-link` to `PATH`.

## AUR

Arch Linux users can install or update the stable release from AUR:

```bash
paru -S pw-duck
```

If `pw-duck` is already installed, the normal system update is enough after the AUR package has been updated:

```bash
paru -Syu
```

Use the stable package `pw-duck` for normal use. A `pw-duck-git` package, if installed, follows the current Git branch instead of the tagged release and is meant for development/testing.

The normal AUR package `pw-duck` is a full desktop build and uses:

```bash
cargo build --release --locked --features gui
cargo test --release --locked --features gui
```

GTK4 is therefore a hard dependency for the AUR package, and `Tuner: open` works by default. The non-GUI build remains useful for development, tests, and intentional headless builds.

For future releases, update the AUR checksum after the final GitHub tag exists:

```bash
./scripts/update-aur-checksum.sh
```

The script downloads `https://github.com/geri1701/pw-duck/archive/refs/tags/v${pkgver}.tar.gz`, writes the SHA-256 checksum to `PKGBUILD` and `.SRCINFO`, and exits deliberately while the tag is not yet published. For `v0.2.0`, the checksum is already set.

## Troubleshooting

### No tray icon visible

- KDE Plasma should support SNI natively.
- GNOME requires an active AppIndicator/KStatusNotifierItem extension.
- If a tray host caches old icons, fully stop the old tray process and start it again.

### No suitable source visible

- The voice app must currently produce a playback stream, usually by being in a call/channel.
- `sources` lists playback streams; microphone sources are not selected.

### Ducking does not react

- Check that the correct voice source is selected.
- Increase sensitivity in the tuner.
- Remember: 0% sensitivity means VAD is off.
- Check that PipeWire/PulseAudio compatibility is running and `pactl` can see streams:

  ```bash
  nix run .# -- status
  ```

### App streams do not disappear after stopping

That can be intentional in a failure path: if streams cannot safely be moved away from the virtual sink, the virtual sink is not destroyed. This avoids killing active applications by destroying the sink underneath them.
