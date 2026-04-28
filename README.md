# pw-duck

`pw-duck` ist eine kleine Linux-Tray-App, die nicht-Voice-Audio automatisch leiser macht, sobald eine konfigurierte Remote-Voice-Quelle aktiv ist.

Der wichtigste Punkt: Die Erkennung läuft auf dem **eingehenden Voice-Stream** der gewählten Anwendung, nicht auf dem lokalen Mikrofon.

## Funktionsprinzip

- Eine Voice-Quelle wird einmal ausgewählt, z. B. Discord/WebRTC.
- Nicht-Voice-Playback-Streams werden während aktivem Ducking über einen virtuellen PipeWire-Sammel-Sink geroutet.
- Die App misst den Pegel der konfigurierten Remote-Voice-Quelle.
- Sobald dort Sprache erkannt wird, wird der Sammelpfad auf die konfigurierte Ducking-Lautstärke reduziert.
- Beim Stoppen wird best-effort sauber zurückgeroutet. Wenn ein Stream nicht sicher zurückbewegt werden kann, bleibt der virtuelle Sink absichtlich erhalten, damit laufende Apps wie Browser oder Spiele nicht beendet werden.

## Voraussetzungen

Zur Laufzeit wird ein moderner Linux-Audio-Desktop erwartet:

- PipeWire mit WirePlumber oder kompatiblem Session-Manager
- PulseAudio-Kompatibilität (`pactl` muss gegen PipeWire/Pulse funktionieren)
- `pw-link` aus PipeWire
- ein StatusNotifierItem-/SNI-Tray-Host
- GTK4 nur für das optionale grafische Regler-Fenster

Mit dem Nix-Paket werden die direkt benötigten Programmabhängigkeiten und Wrapper-Pfade mitgeliefert. PipeWire/WirePlumber und der Tray-Host bleiben Desktop-/Systemdienste des Zielsystems. Der normale Rust-Default-Build enthält die GTK-GUI bewusst nicht; sie wird über das Cargo-Feature `gui` aktiviert.

### Desktop-Unterstützung

- **KDE Plasma:** primäres Ziel, SNI wird nativ unterstützt.
- **GNOME:** benötigt eine AppIndicator-/KStatusNotifierItem-Erweiterung, z. B. „AppIndicator and KStatusNotifierItem Support“.
- Andere SNI-Hosts können funktionieren, unterscheiden sich aber teils bei Klick- und Menüverhalten.

## Starten mit Nix

Aus dem Projektverzeichnis:

```bash
nix run .#tray
```

Regler-GUI direkt öffnen:

```bash
nix run .#tune-gui
```

Terminal-Tuner als Fallback/Debug-Weg:

```bash
nix run .#tune
```

Ohne Subcommand startet `pw-duck` die Tray-App. Allgemeine CLI-Kommandos werden explizit aufgerufen:

```bash
nix run .#
nix run .# -- status
nix run .# -- sources
nix run .# -- select-source <sink-input-index>
nix run .# -- config-path
```

## Erstbenutzung

1. Voice-Anwendung starten und einem Voice-/Call-Kanal beitreten, damit ihr Playback-Stream sichtbar ist.
2. Tray starten:

   ```bash
   nix run .#tray
   ```

3. Im Tray-Menü unter `Steuerung:` → `Quelle: wählen` den passenden Voice-Stream auswählen.
4. `Ducking` einschalten.
5. Bei Bedarf `Regler: öffnen` verwenden.

Alternativ per CLI:

```bash
nix run .# -- sources
nix run .# -- select-source <sink-input-index>
nix run .#tray
```

Achtung: Wenn eine Quelle als `#546` angezeigt wird, im Shell-Befehl nur die Zahl verwenden:

```bash
nix run .# -- select-source 546
```

## Tray-Menü

Das Menü trennt Anzeige und Steuerung bewusst:

```text
Info:
  Ducking: ON/OFF
  Details: ...
  Quelle: ...
  Regler: ...

Steuerung:
  Ducking          # reiner Schalter
  Regler: öffnen  # im Nicht-GUI-Build deaktiviert
  Quelle: wählen

Beenden
```

Der sichtbare Hauptzustand ist absichtlich binär:

- `Ducking OFF`
- `Ducking ON`

Interne Zustände wie Warten, Starten, Neutral, Geduckt oder Fehler stehen nur in den Details.

## Regler

Das GTK-Fenster `tune-gui` bietet drei Werte:

- **Empfindlichkeit**: 0 % deaktiviert VAD vollständig; 100 % ist sehr empfindlich.
- **Ducking-Lautstärke**: Ziel-Lautstärke für Nicht-Voice-Audio während Remote-Sprache aktiv ist, in 1-%-Schritten.
- **Hold**: Nachlaufzeit in Millisekunden, bevor Ducking nach Sprachende gelöst wird.

Die Werte werden sofort in der Konfiguration gespeichert und von einem laufenden Tray-Prozess live übernommen. In Builds ohne Cargo-Feature `gui` bleibt `pw-duck tune` als Terminal-Regler verfügbar; der Tray-Menüpunkt für die GUI ist dort deaktiviert.

## Konfiguration

Die Konfiguration liegt unter:

```text
~/.config/pw-duck/config.toml
```

Gespeichert werden:

- `duck_percent`
- `vad_threshold`
- `hold_ms`
- die stabile Identität der gewählten Voice-Quelle

Nicht persistent gespeichert werden Laufzeitdetails wie `Ducking ON/OFF`, virtuelle Sink-Namen, aktuell geroutete Streams oder VAD-Zustand.

## Entwicklung

Dieses Projekt braucht native PipeWire-/pkg-config-Bibliotheken. Die GTK-GUI ist optional und hängt am Cargo-Feature `gui`.

Nicht-GUI-Pfade bauen ohne GTK:

```bash
direnv exec . cargo check
direnv exec . cargo test
direnv exec . cargo run -- status
direnv exec . cargo run -- sources
```

GUI-/Tray-Entwicklung mit grafischem Regler:

```bash
direnv exec . cargo run --features gui
direnv exec . cargo run --features gui -- tray
direnv exec . cargo run --features gui -- tune-gui
direnv exec . cargo check --features gui
```

Oder interaktiv:

```bash
nix develop
cargo run --features gui
```

Nicht verwenden:

```bash
cargo run -- tray
```

wenn die Shell dabei meldet:

```text
Command 'cargo' not found; attempting execution with nix run...
```

Dann wird nur `cargo` ad hoc geholt, aber nicht die benötigte PipeWire-/pkg-config-Entwicklungsumgebung. Typische Folge sind fehlende `.pc`-Dateien wie `pipewire-0.3.pc`; beim GUI-Feature zusätzlich `glib-2.0.pc`, `gtk4.pc`, `cairo.pc` usw.

## Packaging

Build:

```bash
nix build .#
```

Das Paket installiert:

```text
bin/pw-duck
share/applications/pw-duck.desktop
share/icons/hicolor/.../apps/pw-duck.png
share/icons/hicolor/.../apps/pw-duck-symbolic.png
share/doc/pw-duck/README.md
share/doc/pw-duck/LICENSE
```

Flake-App-Outputs:

```text
.#          pw-duck, startet ohne Subcommand die Tray-App
.#tray      pw-duck tray
.#tune-gui  pw-duck tune-gui
.#tune      pw-duck tune
```

Das Nix-Paket baut mit aktiviertem `gui`-Feature, installiert also auch das grafische Regler-Fenster. Der Nix-Wrapper setzt den `PATH` so, dass `pactl` und `pw-link` aus den Paketabhängigkeiten gefunden werden.

### AUR

Das normale AUR-Paket `pw-duck` soll ebenfalls als vollständiger Desktop-Build ausgeliefert werden. Das mitgelieferte `PKGBUILD` baut deshalb explizit mit:

```bash
cargo build --release --locked --features gui
```

Damit ist GTK4 im AUR-Paket eine harte Abhängigkeit und `Regler: öffnen` funktioniert standardmäßig. Der Nicht-GUI-Build bleibt nur für Entwicklung, Tests oder bewusst headless genutzte Builds gedacht.

Vor dem Upload ins AUR muss nach dem finalen GitHub-Tag die `sha256sums` in `PKGBUILD`/`.SRCINFO` durch die echte Release-Tarball-Prüfsumme ersetzt werden:

```bash
./scripts/update-aur-checksum.sh
```

Der Befehl lädt `https://github.com/geri1701/pw-duck/archive/refs/tags/v${pkgver}.tar.gz`, trägt die SHA-256-Prüfsumme in `PKGBUILD` und `.SRCINFO` ein und bricht bewusst ab, solange der Tag noch nicht veröffentlicht ist.

## Troubleshooting

### Kein Tray-Icon sichtbar

- Unter KDE Plasma sollte SNI nativ funktionieren.
- Unter GNOME muss eine AppIndicator-/KStatusNotifierItem-Erweiterung aktiv sein.
- Falls ein Tray-Host alte Icons cached, die laufende alte Tray-Instanz wirklich beenden und neu starten.

### Keine passende Quelle sichtbar

- Die Voice-App muss gerade einen Playback-Stream erzeugen, also typischerweise in einem Call/Voice-Kanal sein.
- `sources` zeigt Playback-Streams; es werden keine Mikrofonquellen ausgewählt.

### Ducking reagiert nicht

- Prüfen, ob die richtige Voice-Quelle gewählt wurde.
- Empfindlichkeit im Regler erhöhen.
- Beachten: 0 % Empfindlichkeit bedeutet VAD aus.
- Prüfen, ob PipeWire/PulseAudio-Kompatibilität läuft und `pactl` Streams sieht:

  ```bash
  nix run .# -- status
  ```

### App-Streams verschwinden beim Stoppen nicht

Das ist Absicht im Fehlerfall: Wenn Streams nicht sicher vom virtuellen Sink zurückbewegt werden können, wird der virtuelle Sink nicht zerstört. Das verhindert, dass laufende Anwendungen durch einen harten Sink-Destroy beendet werden.
