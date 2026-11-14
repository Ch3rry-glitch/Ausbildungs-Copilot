# Ausbildungs-Cockpit

Windows-App für Auszubildende (Kaufmann/-frau im E-Commerce): eine zweite, schnellere Oberfläche auf
den eigenen **Obsidian-Lernvault**. Unterrichtsnotizen, Aufgaben, Berichtsheft und – das Kernstück –
Logbucheinträge für Moodle. Der Vault bleibt die einzige Wahrheit; Obsidian funktioniert parallel weiter.

> **Stand: M0 (Gerüst).** Vault wählen oder Starter-Vault anlegen, Notizen je Ordner ansehen,
> „In Obsidian öffnen“, Live-Aktualisierung bei Änderungen aus Obsidian. Die App **schreibt noch nichts** in den Vault.

## Installation (für Mitschüler)

1. Unter [Releases](https://github.com/Ch3rry-glitch/Ausbildungs-Copilot/releases) die Datei
   `Ausbildungs-Cockpit_…_x64-setup.exe` herunterladen und starten. Es werden keine Administratorrechte gebraucht.
2. Windows SmartScreen warnt eventuell („Der Computer wurde durch Windows geschützt“), weil der Installer
   nicht signiert ist: **Weitere Informationen → Trotzdem ausführen**.
3. Beim ersten Start **„Starter-Vault anlegen“** wählen. Standard ist `Dokumente\Ausbildungs-Vault`;
   vorhandene Dateien werden nie überschrieben.
4. Den Ordner in [Obsidian](https://obsidian.md) mit **„Ordner als Vault öffnen“** öffnen und unter
   *Einstellungen → Community-Plugins* **Dataview** und **Tasks** installieren.
5. In `00 Start/Stundenplan.md` den eigenen Stundenplan eintragen – die App liest ihn von dort.

## Entwicklung

Voraussetzungen: Rust (stable), Node.js LTS, Visual Studio Build Tools (C++), WebView2.

```bash
npm install
npm run tauri dev      # startet die App; Debug-Builds öffnen automatisch fixtures/vault
cd src-tauri && cargo test
npm run tauri build    # Installer: src-tauri/target/release/bundle/nsis/
```

- **Nie gegen den echten Vault entwickeln.** Debug-Builds nutzen einen eigenen Einstellungsordner
  (`%APPDATA%\ausbildungs-cockpit-dev`) und den Testvault `fixtures/vault/`
  (oder den Pfad aus der Umgebungsvariable `COCKPIT_VAULT`).
- Der Starter-Vault liegt in `src-tauri/starter-vault/` und wird ins Programm eingebettet.
  Neu erzeugen: `bash scripts/build-starter-vault.sh "<Pfad zum Referenz-Vault>"`.
- App-Icon neu erzeugen: `powershell -File scripts/make-app-icon.ps1` und danach `npx tauri icon app-icon.png`.
- GitHub Actions (`.github/workflows/build.yml`, folgt in einem eigenen Commit) testet und baut den Installer; ein Tag `v*` veröffentlicht ihn als Release.

## Aufbau

```
src/                      React + TypeScript (Oberfläche, Deutsch)
src-tauri/src/
  model.rs                Datenmodelle (Rust ↔ Oberfläche)
  vault/                  Markdown verstehen: scan, frontmatter, tasks, tags, links, naming, stundenplan
  index.rs                In-Memory-Index (jederzeit verwerfbar)
  watcher.rs              Dateiwächter (notify, 300 ms gebündelt)
  guard.rs                harte Regeln 6, 8, 10 (EK, .obsidian, 10 Privat)
  starter.rs              Starter-Vault anlegen, nie überschreiben
  obsidian.rs             obsidian://-Links
  config.rs               Einstellungen in %APPDATA%\ausbildungs-cockpit
  commands.rs             Tauri-Befehle
src-tauri/tests/          Tests gegen fixtures/vault
fixtures/vault/           Testvault mit absichtlich unsauberem Altbestand
```

## Harte Regeln (Auszug)

Logbuch-Texte landen nie im Vault · keine Selbstbewertung durch die App · keine Diagnosen im
Berichtsheft · keine erfundenen Tätigkeiten · keine Passwörter · EK-Preise verlassen die App nie ·
`.obsidian/` wird nie angefasst · `10 Privat/` ist vom KI-Versand ausgeschlossen.
