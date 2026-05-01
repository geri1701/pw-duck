# pw-duck

`pw-duck` lowers music, games, videos, and other playback while people are speaking in a selected voice-call stream.

It is built for PipeWire desktops and controlled from a tray icon.

## Features

- Automatic ducking for non-call audio
- Voice-call stream selection from running playback streams
- Tray toggle for ducking on/off
- Sensitivity, ducking volume, and hold-time tuning
- Terminal tuner; graphical tuner in GUI builds

## Install

### Arch Linux / AUR

```bash
paru -S pw-duck
```

Then start `pw-duck` from your launcher or run:

```bash
pw-duck
```

### Nix

From this repository:

```bash
nix run .#
```

Other app outputs:

```bash
nix run .#tray
nix run .#tune
nix run .#tune-gui
```

### From source

```bash
cargo build --release
cargo build --release --features gui
```

The build needs Rust plus PipeWire/pkg-config development libraries. The `gui` feature also needs GTK4.

## Requirements

- PipeWire with a compatible session manager, for example WirePlumber
- PulseAudio compatibility (`pactl` must work)
- `pw-link`
- A StatusNotifierItem/SNI tray host
- GTK4 for the graphical tuner

KDE Plasma supports SNI natively. GNOME needs an AppIndicator/KStatusNotifierItem extension.

## First run

1. Join a voice call so the call playback stream exists.
2. Start `pw-duck`.
3. In the tray menu, choose `Source: choose`.
4. Select the stream that contains the call audio.
5. Enable `Ducking`.
6. Adjust `Tuner: open` if needed.

## Commands

```bash
pw-duck                 # start tray
pw-duck tray            # start tray explicitly
pw-duck sources         # list selectable playback streams
pw-duck select-source 5 # select stream by sink-input index
pw-duck status          # show current audio state
pw-duck tune            # terminal tuner
pw-duck tune-gui        # graphical tuner, if built with gui
pw-duck config-path     # print config path
```

If `sources` shows a stream as `#546`, pass only the number:

```bash
pw-duck select-source 546
```

## Tuning

- **Sensitivity:** how easily call speech is detected; `0%` disables detection.
- **Ducking volume:** volume for non-call audio while speech is active.
- **Hold:** delay before normal volume is restored after speech stops.

Settings are saved to:

```text
~/.config/pw-duck/config.toml
```

## How it works

While ducking is enabled, `pw-duck` creates a temporary virtual PipeWire sink and routes non-call playback through it. The selected call stream stays on the normal output path and is used only for detection.

On shutdown, streams are moved back. If that cannot be done safely, the virtual sink is left alive instead of breaking active application audio.

## Troubleshooting

### No tray icon

- On GNOME, enable an AppIndicator/KStatusNotifierItem extension.
- Restart `pw-duck` if your tray host cached an old icon.

### No call stream appears

- Join a call first; many apps create playback streams only while audio is active.
- Run `pw-duck sources` and pick the stream that contains the call audio.

### Ducking does not react

- Check the selected source.
- Increase sensitivity.
- Make sure sensitivity is not `0%`.
- Run `pw-duck status`.

## Development

Use the dev shell:

```bash
direnv exec . cargo fmt --check
direnv exec . cargo check
direnv exec . cargo test
direnv exec . cargo check --features gui
```

Regenerate icons after editing `assets/icons/source/*.png`:

```bash
scripts/generate-icons.sh
```

Build the Nix package:

```bash
nix build .#
```

## License

MIT
