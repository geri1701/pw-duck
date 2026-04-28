# Plan

## Ziel

- [ ] Eine kleine Tray-App für automatisches Ducking planen.
- [ ] Linksklick auf das Tray-Icon toggelt Ducking an/aus.
- [ ] Rechtsklick öffnet ein minimales Menü.
- [ ] Ein minimales Configfile merkt sich die gewählte Voice-/PipeWire-Quelle.
- [ ] Wenn das Gespräch bzw. die gewählte Voice-Quelle endet/verschwindet, endet auch das Ducking.
- [ ] Die Kernlogik bleibt fachlich an **ankommender Remote-Stimme** orientiert, nicht am lokalen Mikrofon.

## Kontext

- Folgeprojekt: `pw-duck`
- Ausgangsbasis ist die fachliche Kernfunktion von `pw-duck`.
- Zentrale Produktanforderung:
  - Ducking reagiert auf den abgespielten Voice-Output der Gegenseite.
  - Nicht auf das lokale Mikrofon oder die eigene Stimme.
- Gewünschte Oberfläche:
  - Tray-Symbol
  - Linksklick = Ducking toggeln
  - Rechtsklick = minimales Menü
- Gewünschte Persistenz:
  - minimale Konfiguration mit gemerkter Quelle
- Laufzeitverhalten:
  - Ende des Gesprächs / Wegfall der Voice-Quelle beendet aktives Ducking
  - Beim Beenden der App endet ebenfalls das Ducking
- Neu gemeldetes Ist-Problem in `pw-duck`:
  - kurzlebige Sound-/Effekt-Streams können während aktivem Ducking erscheinen und wieder verschwinden
  - der wahrscheinlich entscheidende Fehlerfall ist: ein Ziel-Stream beendet sich **während** es geduckt ist
  - dabei kann ein Ziel mit bereits geducktem Wert zurückbleiben
  - der letzte reduzierte Pegel wird offenbar für die nächste Stream-Instanz weiterverwendet
  - bei erneutem Auftauchen wird dann vom schon reduzierten Wert weitergeduckt
  - der Startzeitpunkt des Streams ist wahrscheinlich zweitrangig; kritisch ist sein Ende im geduckten Zustand
  - Ergebnis: Lautstärke driftet stufenweise bis sehr weit nach unten

## Annahmen und offene Punkte

- [x] Die erste Version soll bewusst klein bleiben und keinen Haupt-Dialog brauchen.
- [x] Das Menü soll minimal bleiben:
  - Status anzeigen
  - Ducking an/aus
  - Quelle wählen
  - Beenden
- [x] „Gespräch beendet“ wird für die erste Version einfach behandelt als:
  - gewählte Voice-Quelle/Voice-Stream ist nicht mehr vorhanden
  - dann Lautstärken sofort wiederherstellen
- [x] Wenn die App beendet wird, werden Lautstärken immer wiederhergestellt.
- [x] Das Folgeprojekt muss den bekannten `pw-duck`-Fehler vermeiden, bei dem kurzlebige Ziel-Streams mit geduckter Lautstärke verschwinden und dadurch Lautstärke-Drift entsteht.
- [x] Klein und robust hat Vorrang: Restore soll auf gespeicherten Ursprungspegeln der Ziel-Outputs beruhen, damit der Zustand beim Beenden sauber wiederhergestellt werden kann.
- [x] Ursprungspegel für neu auftauchende Ziel-Streams werden **sofort beim Auftauchen** erfasst, noch bevor sie in die zu duckenden Ziele aufgenommen werden.
- [x] Workaround für unkontrollierbar verschwindende Ziel-Streams: Ursprungspegel nicht nur an die aktuelle Stream-Instanz hängen, sondern für die laufende App-Session zusätzlich an einer stabileren logischen Ziel-Identität festhalten.
- [x] Diese logische Ziel-Identität soll in der ersten Version bewusst aus **stabileren Inhaltsmetadaten** gebildet werden, nicht aus instanzgebundenen Stream-IDs.
- [x] Die Schwere des Problems wird dadurch erklärt, dass der letzte Stream-Pegel offenbar unterhalb von `pw-duck` weiterverwendet wird; ob der Eigentümer davon Discord, PipeWire oder eine andere Schicht ist, bleibt für die erste Planung offen.
- [x] Wir bevorzugen diesmal die strukturell richtige Lösung statt eines verkleideten Workarounds: der virtuelle Sammel-Output wird zur bevorzugten Architektur, auch wenn er in der Systemintegration aufwendiger ist.
- [x] Das Config-Format soll klein und lesbar bleiben; bevorzugt TOML.
- [x] Die gemerkte Quelle soll über mehrere Merkmale identifiziert werden:
  - bevorzugt `object.serial`, wenn stabil wiederauffindbar
  - zusätzlich `node.name`
  - zusätzlich Label/App-Metadaten als Fallback zum Wiederfinden
- [x] Ducking ohne gemerkte Quelle soll nicht einfach „blind“ aktiv werden; stattdessen braucht es zuerst eine Auswahl.
- [ ] Noch offen ist, wie die Quellenwahl ohne großes Fenster am einfachsten aussieht:
  - rein über Menüeinträge
  - oder kleiner einmaliger CLI-/Popup-Fallback
- [x] „Aktiviert“ bleibt bei fehlender Quelle als **Wartezustand** erhalten; aktive Routing-/Ducking-Eingriffe werden in diesem Zustand jedoch vollständig abgebaut.
- [x] Als bevorzugte robuste Architektur steht jetzt fest:
  - alle Output-Audioquellen außer der Ducking-/Voice-Quelle in einen einzigen virtuellen Output zusammenführen
  - Ducking nur auf dieser virtuellen Zwischenebene anwenden
  - beim Ende dieser virtuellen Ebene soll der ursprüngliche Zustand wiederhergestellt sein
- [x] Befund zur `pipewire`-Crate: Sie bietet brauchbare Low-Level-Bausteine, aber keine fertige High-Level-Abstraktion für „virtueller Sammel-Output plus Routing“.

## Arbeitsschritte

- [x] Fachliche Invarianten aus `pw-duck` für das Folgeprojekt festziehen.
- [x] Minimales UX-Modell für Tray-App definieren:
  - [x] Zustände
  - [x] Klickverhalten
  - [x] Menüeinträge
  - [x] Sichtbares Feedback im Tray
- [x] Laufzeitmodell grob definieren:
  - [x] Quelle geladen/keine Quelle
  - [x] Ducking aktiv/inaktiv
  - [x] Voice-Quelle vorhanden/verschwunden
  - [x] Wiederherstellung der Lautstärken beim Ende
- [x] Minimales Config-Modell grob festlegen:
  - [x] Speicherort
  - [x] Format
  - [x] gemerkte Felder
- [x] Technische Optionen für Linux-Tray + PipeWire + Eventloop einfach gegeneinander abwägen.
- [x] Entscheiden, welche Teile aus `pw-duck` direkt übernehmbar sind und welche neu zugeschnitten werden müssen.
- [x] Eine kleine Implementierungsreihenfolge festlegen.

### Bevorzugte Zielarchitektur

- [x] Tray über `ksni`, um Linksklick (`activate`) und minimales Menü ohne großes GTK-Fenster abzubilden.
- [x] Eine kleine zentrale `AppState`-Struktur als Single Source of Truth für:
  - [x] user_enabled
  - [x] source_configured
  - [x] source_present
  - [x] voice_active
  - [x] duck_applied
  - [x] zuletzt bekannte Quellmetadaten
- [x] PipeWire-/Ducking-Logik als Hintergrunddienst im selben Prozess, logisch getrennt von der Tray-Schicht.
- [x] Keine TUI/GUI aus `pw-duck` übernehmen.
- [x] Minimaler Tray-Status:
  - [x] anderes Icon oder Titel je nach an/aus/wartend
  - [x] Menütext mit aktuellem Zustand
- [x] Minimales Configfile, bevorzugt unter XDG-Config, z. B. `~/.config/pw-duck/config.toml`.
- [x] Gemerkte Quelle als kleine Struktur mit stabilen PipeWire-Merkmalen plus Anzeige-Label.
- [x] Statt Ziel-Stream-Lautstärken direkt zu verändern, alle Nicht-Voice-Outputs in einen virtuellen Sammel-Output routen.
- [x] Nur diesen virtuellen Sammel-Output ducken, nicht die einzelnen Quell-Streams selbst.
- [x] Die Voice-/Ducking-Quelle bleibt außerhalb dieses Sammel-Outputs und dient nur als Trigger.
- [x] Wenn der virtuelle Sammel-Output beendet oder entfernt wird, soll das System wieder wie vorher klingen, ohne dass einzelne Stream-Pegel dauerhaft verändert wurden.
- [x] Die erste Umsetzung soll den Routing-/Lifecycle-Pfad dieses Sammel-Outputs minimieren und nicht schon eine allgemeine Routing-Engine werden.
- [x] Die erste Version nimmt bewusst **einen primären realen Ziel-Output** an, nicht beliebig viele getrennte physische Ziele.
- [x] Neue Nicht-Voice-Streams werden im aktiven Routing-Zustand direkt in den Sammel-Output verschoben.
- [x] Verschwindet die Voice-Quelle, fällt die App in einen Wartezustand zurück und baut das aktive Routing vollständig ab.

### Fallback-/Referenzpfad: Direkt-Ducking mit Session-Workaround

- [x] Restore-Logik aus `pw-duck` kann als Referenz für den Hintergrundkern dienen.
- [x] Restore-Basis wäre im Fallback bewusst einfach:
  - [x] Ursprungspegel der Ziel-Outputs explizit speichern
  - [x] Ursprungspegel neuer Ziel-Streams sofort beim Auftauchen erfassen
  - [x] erst danach den Stream in die duckbaren Ziele aufnehmen
  - [x] Ducking immer relativ zu diesen gespeicherten Ursprungspegeln anwenden
  - [x] beim Ende/Beenden auf diese gespeicherten Werte restaurieren
  - [x] wenn ein Ziel-Stream im geduckten Zustand verschwindet, seinen letzten bekannten Ursprungspegel **nicht sofort vergessen**
  - [x] taucht die gleiche logische Zielquelle später erneut auf, wird ihr Ursprungspegel aus dem Session-Speicher wiederverwendet statt blind vom aktuell schon reduzierten Wert zu starten
- [x] Session-Key für kurzlebige Ziel-Streams bewusst einfach und robust halten:
  - [x] bevorzugt aus `application.process.binary` + `application.name` + `media.name`
  - [x] `media.role` und `media.class` als zusätzliche Unterscheidung
  - [x] `node.name` nur ergänzend verwenden, wenn sinnvoll/stabil vorhanden
  - [x] **nicht** aus `id`, `object.serial`, `client.id` oder `pid`, weil diese zu instanznah und flüchtig sind

### Minimaler Routing-/Lifecycle-Plan

- [x] **Zustände**
  - [x] `Idle`: User hat Ducking nicht aktiviert; kein virtueller Sammel-Output, kein Routing-Eingriff.
  - [x] `WaitingForSource`: User will Ducking, aber die konfigurierte Voice-Quelle ist gerade nicht vorhanden; kein Routing-Eingriff aktiv.
  - [x] `RoutedNeutral`: Voice-Quelle vorhanden, virtueller Sammel-Output aktiv, Nicht-Voice-Streams sind dorthin geroutet, Gain = 1.0.
  - [x] `RoutedDucked`: wie `RoutedNeutral`, aber Sammel-Output ist abgesenkt.
- [x] **Aktivierungsablauf**
  - [x] Linksklick setzt `user_enabled` um.
  - [x] Wenn keine konfigurierte Quelle existiert: in `WaitingForSource` bleiben und im Tray Auswahl ermöglichen; kein Routing-Eingriff und kein Fehlerzustand.
  - [x] Wenn Quelle konfiguriert, aber nicht vorhanden: in `WaitingForSource` gehen.
  - [x] Wenn Quelle vorhanden: virtuellen Sammel-Output erzeugen/aktivieren, primären realen Ziel-Output bestimmen, passende Nicht-Voice-Streams umhängen, dann mit neutralem Gain starten (`RoutedNeutral`).
- [x] **Routing-Regeln**
  - [x] Voice-Quelle selbst wird nie in den Sammel-Output umgehängt.
  - [x] Normale Nicht-Voice-Playback-Streams werden in den Sammel-Output umgehängt.
  - [x] Neu auftauchende Nicht-Voice-Streams werden bei aktivem Routing sofort ebenfalls dorthin verschoben.
  - [x] Die erste Version versucht nicht, eine allgemeine Multi-Sink- oder Per-App-Routing-Matrix zu verwalten.
- [x] **Ducking-Regeln**
  - [x] VAD reagiert ausschließlich auf die konfigurierte Remote-Voice-Quelle.
  - [x] Ducking verändert nur den Gain des Sammel-Outputs.
  - [x] Einzelne Ziel-Streams werden in dieser Architektur nicht im Pegel verändert.
- [x] **Ende-/Teardown-Regeln**
  - [x] Wenn User deaktiviert: Sammel-Output auf neutral setzen, umgehängte Streams zurück auf den realen Ziel-Output bzw. den einfachen Standardpfad geben, Sammel-Output entfernen, in `Idle` gehen.
  - [x] Wenn die Voice-Quelle verschwindet: denselben Teardown ausführen, aber `user_enabled` beibehalten und in `WaitingForSource` gehen.
  - [x] Wenn die App beendet oder abstürzt: best-effort denselben Teardown versuchen.
  - [x] Weil die Quell-Streams selbst nie geduckt werden, soll der Drift-Fall kurzlebiger Streams dadurch strukturell vermieden werden.
- [x] **Bewusste Vereinfachung für v1**
  - [x] Ein primärer realer Ziel-Output zur Laufzeit genügt.
  - [x] Keine Garantie für perfekte Wiederherstellung exotischer manueller Sonderroutings.
  - [x] Fokus ist robuste Rückkehr zu einem normalen Standardzustand statt vollständiger universeller Audio-Topologie-Verwaltung.

### Befund: `pipewire`-Crate und Routing-Primitive

- [x] Die `pipewire`-Crate kann die bereits aus `pw-duck` bekannte Basis gut tragen:
  - [x] Mainloop, Context, Core, Registry
  - [x] Beobachten von Nodes/Ports/Links/Factories/Metadata
  - [x] Erzeugen eigener Streams
  - [x] Erzeugen und Zerstören von Remote-Objekten über `Core::create_object` / `destroy_object`
  - [x] Erzeugen von Links über `link-factory`
  - [x] Erzeugen von Nodes über vorhandene Factories wie `adapter`
- [x] PipeWire selbst hat passende Bausteine für einen virtuellen Knoten:
  - [x] `adapter`-Factory ist vorhanden
  - [x] `support.null-audio-sink` ist als Beispielprimitive für virtuelle Nodes dokumentiert
  - [x] `link-factory` ist vorhanden
  - [x] `pw-loopback`, `pw-link`, `pw-cli`, `pw-metadata`, `wpctl` und `pactl` sind lokal verfügbar und können für PoC/Diagnose dienen
- [x] Die `pipewire`-Crate bietet aber keine fertige High-Level-Funktion für:
  - [x] „virtuellen Sammel-Output erstellen“
  - [x] „alle Nicht-Voice-Streams automatisch dorthin routen“
  - [x] „Loopback zum realen Ziel-Output verwalten“
  - [x] „WirePlumber-Policy kontrolliert überstimmen“
- [x] `pw_filter` wäre in der C-API für komplexere Filter/Gain-Knoten relevant, ist in `pipewire` 0.9.2 aber nicht als komfortable Rust-API gewrappt; dafür gäbe es höchstens `pipewire-sys`/FFI.
- [x] Für die erste Umsetzung ist deshalb der kleinste realistische Weg:
  - [x] zuerst einen kleinen PoC mit PipeWire-eigenen Primitiven (`adapter`/virtueller Node + Links/Loopback) validieren
  - [x] danach entscheiden, ob die App diese Primitiven direkt über `pipewire`-Crate-Objekte verwaltet oder gezielt vorhandene Tools/Module als Übergang nutzt
- [x] Vorläufige technische Präferenz:
  - [x] `pipewire`-Crate für Beobachtung, Voice-Capture, VAD und möglichst auch Objekt-/Link-Lifecycle
  - [x] `pw-loopback`/`pw-cli`/`pactl` nur als Diagnose- oder PoC-Hilfen, nicht vorschnell als dauerhafte Produktabhängigkeit

### Vorgeschlagene erste Implementierungsreihenfolge

- [x] Cargo-Projekt + Basisschichten anlegen
- [x] Config laden/speichern
- [x] AppState + Tray-Grundgerüst
- [x] PipeWire-Streambeobachtung ohne Ducking
- [x] gemerkte Quelle wiederfinden
- [x] Capture + VAD aus `pw-duck` übernehmen/anpassen
- [x] Minimalen virtuellen Sammel-Output planerisch festziehen:
  - [x] wie er erzeugt/verwaltet wird
  - [x] wie Nicht-Voice-Streams dort landen
  - [x] wie die Voice-Quelle explizit draußen bleibt
  - [x] wie Teardown/Beenden den Ursprungszustand wiederherstellt
- [x] Vor technischer Umsetzung die kleinste konkrete PipeWire-/WirePlumber-Primitive untersuchen: `pipewire`-Crate bietet Low-Level-Bausteine, aber keinen fertigen Sammel-Output; nächster Schritt ist ein kleiner mutierender PoC mit virtuellem Node + Routing.
- [x] Mutierenden PoC durchführen: virtuellen Sammel-Output testweise erzeugen, Nicht-Voice-Teststream dorthin routen, Monitor-Ports zum realen Sink linken, Teardown prüfen.
- [x] Korrektur aus Praxistest: Der Regler des virtuellen Sinks wirkte bei direkter Monitor-Port-Verlinkung nicht zuverlässig, weil der virtuelle Sink mit `monitor.passthrough=true` erzeugt wurde. Diese Eigenschaft wird für den Ducking-Sink nicht gesetzt; direkter Monitor-Link bleibt der kleinste Pfad.
- [x] Tray-Aktionen mit der Routing-/Ducking-Engine koppeln
- [x] Erster Tray-Pfad umgesetzt:
  - [x] `tray`-Subcommand startet eine StatusNotifier-/SNI-Tray-App über `ksni`.
  - [x] Linksklick toggelt Ducking Start/Stop.
  - [x] Kontextmenü enthält Start/Stop, aktuellen Status, aktuelle Quelle, dynamische Quellenwahl aus sichtbaren Playback-Streams und Beenden.
  - [x] Tray startet die bestätigte Routing-/VAD-Engine in einem Worker-Thread und führt beim Stop/Quit den bestehenden sicheren Teardown aus.
  - [x] Tray schützt gegen Doppelinstanzen per Runtime-Lock und ignoriert kurz nach Menüaktionen nachgelagerte `activate`-Events, damit Quellenwahl nicht versehentlich Ducking/zweite Worker triggert.
  - [x] Tray bleibt während der Laufzeit stabil `Active`, damit ashell keinen `Passive → Active`-Sichtbarkeitswechsel als neue Geisterinstanz wirken lässt; vor dem DBus-Shutdown setzt es sich explizit auf `Passive` und wartet kurz.
  - [x] `ksni` läuft grundsätzlich spec-clean mit well-known SNI-Name. Ausnahme: Wenn der SNI-Watcher `ashell` ist, wird automatisch der Unique-Name-Modus genutzt, weil ashell im Test nach Start → Ducking ON → OFF sonst einen stale `:1.x/StatusNotifierItem`-Ghost behält. Per `PW_DUCK_SNI_UNIQUE_NAME=0/1` kann der Modus explizit überschrieben werden.
  - [x] Sichtbarer Tray-Status ist bewusst nur binär: `Ducking OFF` oder `Ducking ON`; auch die erste Menüzeile zeigt nur diesen binären Status, interne Detailzustände bleiben nur als Menü-/Tooltip-Details erhalten.
  - [x] Tray-Toggle ist bewusst einfach angebunden: `Activate`, `SecondaryActivate` und der explizite Menüpunkt rufen dieselbe zentrale Toggle-Funktion auf; das Menü zeigt oben den binären Status read-only.
  - [x] Tray-Menü ist strukturiert: reine Anzeigen stehen im Abschnitt `Info:`, interaktive Befehle im Abschnitt `Steuerung:`; Separatoren trennen die Gruppen sichtbar. Im Steuerungsbereich ist `Ducking` ein reiner Checkmark-Schalter statt eines Status-/Aktionssatzes; die übrigen Befehle nutzen `Name: Aktion`, z. B. `Regler: öffnen`, `Quelle: wählen`. Menüeinträge tragen bewusst keine eigenen Icons mehr; das Duck-Symbol erscheint nur als eigentliches Tray-Symbol.
  - [x] Aus dem Ducking-Logo wurden PNG-Icons per ImageMagick erzeugt: App-Icon mit dunklem abgerundetem Hintergrund sowie transparente Symbol-Variante im hicolor-Layout unter `assets/icons/`.
  - [x] Tray-Icon-Pixmaps verwenden bewusst die transparente Symbol-Variante statt des App-Icons, damit SNI-Hosts beim Neustart nicht zwischen App-Icon, Symbol-Icon, Theme-Cache oder unterschiedlich skalierten Hintergrund-Icons wechseln.
  - [x] Das Tray-Symbol liefert zusätzlich eingebettete SNI-`IconPixmap`-Daten aus `assets/icons/pixmap/`, damit der SNI-Host nicht weiter ein altes Icon aus Theme-/Host-Cache rendert.
  - [x] Flake-Apps für robuste Starts ohne Devshell ergänzt: `nix run .#tray`, `nix run .#tune-gui`, `nix run .#tune`. Ein nacktes `cargo` aus dem Nix-command-not-found-Fallback ist bewusst kein unterstützter Buildpfad, weil dabei GTK/PipeWire/pkg-config-Inputs fehlen. Die App-Outputs haben eigene Metadaten. Der Devshell setzt `PKG_CONFIG_PATH` nicht mehr hart auf PipeWire, damit die pkg-config-Setup-Hooks auch GTK/GLib/GIO/Pango/Cairo sichtbar machen.
  - [x] Icons sind eingebunden: Tray und GTK-Reglerfenster nutzen `pw-duck` mit zusätzlichem Icon-Theme-Pfad; die transparente `pw-duck-symbolic`-Variante wird mitinstalliert. Das Nix-Paket installiert hicolor-Icons plus validierte Desktop-Datei, README und MIT-Lizenz; die Nix-Quelle filtert generierte Verzeichnisse wie `target/` und `.direnv/` heraus.
  - [x] Release-Name konsolidiert: Cargo-Paket, Binary, CLI-Name, Nix-Paket/App-Outputs, Desktop-Datei, Icon-Namen, SNI-ID, Runtime-Lock, Config-Pfad, Env-Overrides und Dokumentation heißen jetzt konsequent `pw-duck`; Version ist `0.2.0` für die neue Hauptversion des bestehenden Projekts. Ohne Subcommand startet `pw-duck` jetzt die Tray-App; Diagnose bleibt explizit über `pw-duck status` verfügbar.
  - [x] AUR-Checksum-Finalisierung abgesichert: `scripts/update-aur-checksum.sh` lädt nach Veröffentlichung des GitHub-Tags den Release-Tarball, trägt die echte SHA-256-Prüfsumme in `PKGBUILD` und `.SRCINFO` ein und bricht bewusst ab, solange der Tag noch fehlt.
- [x] Erste einfache Quellenwahl/-Persistenz per CLI angelegt:
  - [x] `sources` listet aktuelle Playback-Streams read-only
  - [x] `select-source <sink-input-index>` speichert stabile Metadaten als TOML-Voice-Quelle
  - [x] Discord/WEBRTC-Varianten werden toleranter gematcht: exakte gespeicherte Identität gewinnt, zusätzlich gilt gleiches `application.process.binary` plus Voice-/Discord-Hinweis als kompatible Voice-Quelle.
- [x] Quelle über Tray-Menü wählen und speichern
- [x] Erste kleine Rust-Routing-Schicht angelegt:
  - [x] Config-/Identitätsmodell für Voice-Quelle
  - [x] read-only Status über `pactl --format=json`
  - [x] virtueller Sink per `pipewire`-Crate (`adapter` + `support.null-audio-sink`)
  - [x] virtueller Ducking-Sink ist passiv und mit niedriger Policy-Priorität markiert; er bleibt technisch ein echter `Audio/Sink`, weil Pulse-Streams gezielt dorthin verschoben werden müssen.
  - [x] Monitor-Links per `pw-link` vom virtuellen Sink zum realen Sink
  - [x] Nicht-Voice-Playback-Streams per `pactl move-sink-input` in den virtuellen Sink verschieben
  - [x] Ducking-Gain per `pactl set-source-volume <virtual-sink>.monitor`; der virtuelle Sink selbst bleibt nur Sammelpunkt, sein Sink-Volume ist im Monitor-Link-Pfad nicht verlässlich genug.
  - [x] Während einer laufenden `route-once`-Session werden neu auftauchende Nicht-Voice-Playback-Streams per kleinem Polling nachgeroutet.
  - [x] Der vor Session-Start erkannte System-Default-Sink wird beim Erzeugen/Entfernen des virtuellen Sinks explizit wiederhergestellt; der reale Ducking-Ziel-Sink kann davon abweichen, wenn die Voice-Quelle auf einem anderen Output läuft.
  - [x] Wenn die konfigurierte Voice-Quelle beim Start sichtbar ist, bestimmt ihr aktueller Sink den realen Ziel-Output; das verhindert Fehlrouting, wenn Pulse/WirePlumber den Default-Sink bereits auf ein anderes Gerät gesetzt hat.
  - [x] Verschobene Streams werden beim Teardown auf ihren ursprünglich gemerkten Sink-Index zurückgeschoben.
  - [x] Teardown ist fail-safe: der virtuelle Sink wird erst zerstört, wenn keine Sink-Inputs mehr daran hängen; andernfalls bleibt er absichtlich bestehen, um den laufenden Audiostream nicht zu beenden.
  - [x] Fail-safe ist gegen RAII-Drop abgesichert: Wenn der virtuelle Sink wegen verbliebener Inputs nicht zerstört werden darf, wird sein expliziter Destroy im `VirtualSink`-Drop entschärft (`abandon`).
  - [x] Streams, die bereits am virtuellen Sink hängen, aber nicht in `moved_inputs` stehen, werden beim Nachrouting/Teardown erkannt und auf den realen Sink zurückgeführt.
  - [x] RAII-/Drop-Teardown für Restore, Unlink und Sink-Destroy
  - [x] mutierender Pfad nur als expliziter Debug-Befehl `route-once --yes-really-route`
- [x] Dauerlauf-Routing per `route --yes-really-route` angelegt:
  - [x] läuft bis `Ctrl+C`
  - [x] routet neu auftauchende Nicht-Voice-Streams periodisch nach
  - [x] captured die konfigurierte Voice-Quelle per PipeWire-Monitor-Capture und schaltet Ducking per einfacher RMS-VAD-Logik nur bei Voice-Aktivität ein
  - [x] VAD-Parameter für den ersten Lauf: Threshold CLI-Option `--vad-threshold`, Attack 40 ms, Hold 700 ms
  - [x] VAD nutzt Hysterese gegen Discord/WebRTC-Ruhepegel: `--vad-threshold` ist die Basis-/Noise-Schwelle, Aktivierung erfolgt erst ab ca. `2x`, Freigabe unter ca. `0.75x`.
  - [x] Drei Regler aus dem alten TUI sind wieder vorhanden: Empfindlichkeit (`vad_threshold`), Ducking-Lautstärke (`duck_percent`, 1%-Schritte) und Hold-Zeit (`hold_ms`, 0–4000 ms). Für GUI-affine Nutzer gibt es `pw-duck tune-gui` als kleines GTK4-Reglerfenster; die Terminal-TUI `pw-duck tune` bleibt als Fallback/Debug-Pfad erhalten. Die GTK-GUI ist ein optionales Cargo-Feature `gui`, damit Nicht-GUI-Pfade wie `status`, `sources`, `cargo check` und `cargo test` nicht von GTK/pkg-config abhängen; das Nix-Paket und das normale AUR-Paket bauen dieses Feature für die auslieferbare Desktop-Version mit. Das Tray öffnet über „Regler öffnen“ die GUI, wenn das Feature einkompiliert ist; in Nicht-GUI-Builds ist dieser Menüpunkt deaktiviert und erzeugt keine Fehlermeldung im Info-Block. Die GTK-App wird ohne Clap-Subcommand-Argumente gestartet, damit GApplication `tune-gui` nicht als zu öffnende Datei interpretiert; der Tray wartet im Hintergrund auf den GUI-Kindprozess, damit keine Zombies entstehen.
  - [x] Laufendes Ducking synchronisiert Reglerwerte aus `config.toml`, damit Änderungen aus der TUI live wirken.
  - [x] VAD behandelt ausbleibende neue Capture-Frames als Stille, damit ein alter hoher Messwert nicht weiter Ducking auslösen kann.
  - [x] Empfindlichkeit `0%` ist jetzt wirklich VAD aus: bei maximaler Schwelle (`vad_threshold >= 0.2`) aktiviert Ducking nie, auch nicht bei Artefaktpegeln.
  - [x] führt beim Beenden explizit Restore/Teardown aus
  - [x] VAD-Capture nutzt das eindeutige `object.serial` des sichtbaren Voice-Sink-Inputs und fällt auf `node.name` zurück; der zuvor getestete `parec --monitor-stream`-Pfad lieferte hier nicht zuverlässig Frames.
  - [x] Laufendes Ducking prüft die konfigurierte Voice-Quelle weiter auf Präsenz. Verschwindet sie, wird der Stream neu erzeugt oder wird im Tray eine andere Quelle gespeichert, werden VAD und Routing sauber abgebaut und der Worker startet mit der aktuellen Config neu bzw. wartet ohne aktiven virtuellen Sink auf die Rückkehr der Quelle.
  - [x] VAD-Reaktion beschleunigt: aktive Pegelprüfung läuft mit 50 ms Intervall, während teurere Source-/Routing-Prüfungen weiter nur ca. alle 500 ms laufen.
- [ ] Ende- und Fehlerpfade sauber auf Teardown/Restore des Sammel-Outputs führen
- [ ] Nur falls der Routing-Pfad praktisch blockiert oder unverhältnismäßig wird: Fallbackpfad Direkt-Ducking mit Session-Workaround weiterverfolgen
- [ ] Die Pegelpersistenz-Eigentümerschaft nicht vorschnell festschreiben: Discord, PipeWire oder eine andere Schicht nur bei Bedarf später gezielt prüfen.

## Risiken / Auswirkungen

- Falsche Quellenidentität kann dazu führen, dass die gemerkte Quelle später nicht mehr sauber wiedergefunden wird.
- `ksni` ist für eine einfache Linux-Tray-App attraktiv, bleibt aber an die reale Tray-/SNI-Unterstützung der Desktop-Umgebung gebunden.
- Wenn Linksklick, Wartestatus und echtes Ducking nicht sauber getrennt werden, drohen verwirrende Zustände.
- Die Wiederverwendung von `pw-duck`-Teilen senkt Risiko, aber `main.rs` muss dabei logisch zerlegt statt blind kopiert werden.
- Eine zu komplexe Quellenwahl würde das Ziel einer kleinen Tray-App unterlaufen.
- Kurzlebige Output-Streams und Stream-/Device-Neuerzeugung können Restore und Baselines verfälschen, vor allem wenn ein Ziel-Stream im geduckten Zustand endet und dadurch zu spät oder gar nicht restauriert wird.
- Wenn die logische Ziel-Identität zu instabil gewählt wird, kann der Session-Speicher den falschen Ursprungspegel wiederverwenden oder denselben logischen Stream nicht wiedererkennen.
- Wenn Ursprungspegel auf der falschen Ebene oder zum falschen Zeitpunkt erfasst werden, kann auch ein bewusst einfaches Restore-Modell trotzdem driften.
- Wenn eine unterlagerte Schicht den letzten Pegel konserviert, kann ein reiner In-Memory-Workaround Grenzen haben, besonders nach App-Neustart oder ohne zuvor bekannte gute Baseline.
- Der virtuelle Sammel-Output ist die bevorzugte Architektur gegen diese Drift-Klasse, erhöht aber Routing-, Lifecycle- und Integrationsrisiko gegenüber Direkt-Ducking deutlich.
- Der mutierende PoC hat bestätigt, dass ein per `adapter` + `support.null-audio-sink` erzeugter virtueller `Audio/Sink` praktisch als Sammel-Output funktioniert.
- Die Monitor-Ports dieses virtuellen Sinks lassen sich auf den realen Default-Sink routen. Der praktisch bestätigte hörbare Gain-Punkt ist die Monitor-Source (`<virtual-sink>.monitor`) per `pactl set-source-volume`; die virtuelle Sink-Lautstärke selbst ist im direkten Monitor-Link-Pfad nicht verlässlich genug.
- Die `pipewire`-Crate reicht für Low-Level-Lifecycle und Beobachtung aus, nimmt uns aber WirePlumber-Policy und Routing-Semantik nicht vollständig ab.
- Die erste Rust-Schicht kapselt diese Lücke bewusst: Sink-Lifecycle direkt über `pipewire`, Policy-nahe Schritte zunächst klein isoliert über `pactl`/`pw-link`, damit sie später ohne große App-Änderung durch reine PipeWire-/WirePlumber-Aufrufe ersetzt werden können.

## Freigabe

Vor der Umsetzung freizugeben:
- [x] Architektur in der Planung einfach halten.
- [x] „Gespräch beendet“ vorläufig als **Voice-Stream verschwunden / nicht mehr verfügbar** behandeln.
- [x] Die erste Version des Menüs bewusst minimal halten:
  - [x] Ducking an/aus
  - [x] Quelle wählen
  - [x] Beenden
- [x] Nicht zuerst einen verkleideten v1-Workaround priorisieren; bevorzugt wird die virtuelle-Sammel-Output-Architektur.
- [x] Den minimalen Routing-/Lifecycle-Plan für den virtuellen Sammel-Output vor jeder Umsetzung ausarbeiten.
- [x] Die kleinste konkrete technische Primitive wurde praktisch validiert und korrigiert: `adapter` + `support.null-audio-sink` als virtueller `Audio/Sink` ohne `monitor.passthrough=true`, Monitor-Ports per Link zum realen Default-Sink, Stream-Routing per Sink-Move, Gain auf der virtuellen Monitor-Source.
- [x] Mutierender PoC mit virtuellem Sammel-Output und sauberem Teardown wurde durchgeführt.
