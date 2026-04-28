# AGENTS.md

Hinweise für Coding-Agenten in diesem Repository.

## Projekt

`pw-duck` ist eine kleine Linux-Tray-App für Audio-Ducking:

- Die App reagiert auf den **eingehenden Remote-Voice-Playback-Stream** einer gewählten Anwendung.
- Sie reagiert ausdrücklich **nicht** auf das lokale Mikrofon.
- Nicht-Voice-Audio wird während aktivem Ducking über einen virtuellen PipeWire-Sammel-Sink geroutet.
- Geduckt wird der Sammelpfad, nicht einzelne App-Streams.

Primäres Ziel ist KDE Plasma mit SNI/StatusNotifierItem. GNOME braucht eine AppIndicator-/KStatusNotifierItem-Erweiterung.

## Wichtige Invarianten

- Keine Rückkehr zu Per-Stream-Volume-Ducking, außer als bewusst dokumentierter Fallback.
- Voice-Quelle nie in den virtuellen Sammel-Sink routen.
- Teardown ist fail-safe: Streams zuerst zurückbewegen, virtuellen Sink nur zerstören, wenn keine Inputs mehr daran hängen.
- Wenn Streams nicht sicher zurückbewegt werden können, virtuellen Sink stehen lassen statt laufende Apps zu killen.
- Originalen Default-Sink nach Routing-/Teardown-Pfaden wiederherstellen.
- Laufender Tray übernimmt Quellenwechsel aus der Config: aktive Session sauber abbauen und mit aktueller Quelle neu starten/warten.
- Sichtbarer Tray-Hauptstatus bleibt binär: `Ducking OFF` / `Ducking ON`.
- Endnutzer-UI und README sind für den Open-Source-Release auf Englisch; keine neuen deutschen App-Labels einführen, solange keine i18n-Struktur existiert.
- Tray-Menü trennt Info und Controls; `Ducking` im Steuerungsbereich ist ein reiner Schalter.
- Menüeinträge haben bewusst keine eigenen Icons; das Duck-Symbol erscheint nur als Tray-Symbol.
- Ohne Cargo-Feature `gui` darf der Build nicht GTK benötigen; `tune-gui` ist dann nicht verfügbar und der Tray-Menüpunkt bleibt deaktiviert.
- Nix-Paket und normales AUR-Paket sollen die GUI standardmäßig mitbauen.

## Konfiguration und Namen

- Paket/Binary/CLI: `pw-duck`
- Config: `~/.config/pw-duck/config.toml`
- Desktop-Datei: `assets/applications/pw-duck.desktop`
- Icons: `pw-duck`, `pw-duck-symbolic`
- SNI-ID / Runtime-Lock / Env-Overrides verwenden ebenfalls `pw-duck` / `PW_DUCK_*`.

Keine neuen `pw-duck-tray`-Namen einführen.

## Entwicklung auf NixOS

Dieses Repo ist flake-/direnv-basiert. Für projektabhängige Befehle bevorzugt:

```bash
direnv exec . cargo fmt --check
direnv exec . cargo check
direnv exec . cargo test
direnv exec . cargo check --features gui
```

GUI-/Tray-Entwicklung:

```bash
direnv exec . cargo run --features gui
direnv exec . cargo run --features gui -- tray
direnv exec . cargo run --features gui -- tune-gui
```

Nicht-GUI-Pfade müssen ohne GTK bauen:

```bash
direnv exec . cargo check
direnv exec . cargo test
```

Nicht auf nacktes `cargo` außerhalb der Devshell verlassen, wenn Nix command-not-found nur Cargo ad hoc holt; dann fehlen pkg-config-Inputs.

## Nix- und Release-Prüfungen

Vor Release- oder Packaging-Änderungen mindestens:

```bash
direnv exec . cargo fmt --check
direnv exec . cargo check
direnv exec . cargo test
direnv exec . cargo check --features gui
direnv exec . desktop-file-validate assets/applications/pw-duck.desktop
nix flake check --no-build
nix build .# --no-link
nix run .# -- --version
nix run .#tray -- --help
nix run .#tune-gui -- --help
```

## AUR

Das normale AUR-Paket `pw-duck` ist ein vollständiger Desktop-Build und nutzt `--features gui`.

Vor echtem AUR-Upload:

1. GitHub-Tag veröffentlichen, z. B. `v0.2.0`.
2. Danach ausführen:

   ```bash
   ./scripts/update-aur-checksum.sh
   ```

3. Sicherstellen, dass `PKGBUILD` und `.SRCINFO` keine `sha256sums=('SKIP')` mehr enthalten.

`SKIP` ist nur für lokale Vorabtests akzeptabel, nicht für den finalen AUR-Release.

## Arbeitsstil

- Änderungen klein und zielgerichtet halten.
- Vor Funktionsänderungen relevante Pfade lesen: `src/duck.rs`, `src/routing.rs`, `src/tray.rs`, `src/vad.rs`, `src/config.rs`.
- Release-/Dokumentationspolish darf die Audio-Funktion nicht verändern.
- Mutierende Audio-Befehle nur über bestehende explizite Sicherheitsmechanismen wie `--yes-really-route` ausführen.
- Bei Unsicherheit lieber bestehende Tests/Checks erweitern als Architektur umbauen.
