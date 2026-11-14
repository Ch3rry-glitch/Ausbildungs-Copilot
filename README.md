# Ausbildungs-Cockpit

Windows-App für Auszubildende (Kaufmann/-frau im E-Commerce): eine zweite, schnellere Oberfläche auf
den eigenen **Obsidian-Lernvault**. Unterrichtsnotizen, Aufgaben, Berichtsheft und – das Kernstück –
Logbucheinträge für Moodle. Der Vault bleibt die einzige Wahrheit; Obsidian funktioniert parallel weiter.

> **Stand: M1 (Unterricht + Heute).** Startseite „Heute“ mit Stundenplan-Blöcken und fälligen Aufgaben,
> Unterrichtsnotiz aus der Vorlage anlegen (Strg+N), Notizen im Editor bearbeiten und speichern (Strg+S) mit
> Konfliktschutz gegenüber Obsidian. Die App legt nur neue Notizen an und speichert geöffnete – sie löscht,
> verschiebt und benennt nichts um.

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
npm run tauri dev      # startet die App; Debug-Builds öffnen eine Kopie von fixtures/vault unter %TEMP%
cd src-tauri && cargo test
npm run tauri build    # Installer: src-tauri/target/release/bundle/nsis/
```

- **Nie gegen den echten Vault entwickeln.** Debug-Builds nutzen einen eigenen Einstellungsordner
  (`%APPDATA%\ausbildungs-cockpit-dev`) und öffnen eine **Kopie** des Testvaults
  `fixtures/vault/` unter `%TEMP%\ausbildungs-cockpit-dev-vault-<hash>` (oder den Pfad aus der
  Umgebungsvariable `COCKPIT_VAULT`). `<hash>` sind 8 Hex-Zeichen über Pfade und Inhalte der Fixtures:
  Ändern sich die Fixtures, entsteht beim nächsten Start eine frische Kopie. Alte Kopien bleiben liegen
  und werden nie gelöscht. Den genauen Pfad schreibt die App beim Start ins Terminal:
  `[dev] Testvault-Kopie: …`. Ordner mit `.` am Anfang (z. B. `.obsidian/`) werden nicht mitkopiert.
- Ein im Dev-Einstellungsordner gespeicherter Vault hat Vorrang vor der Kopie. Liegt er nicht unter
  `%TEMP%`, warnt das Terminal (`[dev] Dev-Build: gespeicherter Vault liegt nicht unter %TEMP% …`) –
  dann unter *Einstellungen* prüfen, welcher Vault offen ist.
- **Schreiben nur in die Temp-Kopie.** Debug-Builds lehnen Anlegen und Speichern außerhalb von `%TEMP%`
  ab („Entwicklungs-Build: Schreiben ist nur in einer Vault-Kopie unter %TEMP% erlaubt …“). Wer bewusst
  in einen anderen Testordner schreiben will, setzt `COCKPIT_DEV_WRITE=1`. In `fixtures/` und
  `src-tauri/starter-vault/` schreiben Debug-Builds und Tests nie, auch nicht mit `COCKPIT_DEV_WRITE=1`.
  Release-Builds haben diese Sperre nicht.
- **Uhrzeit vorgeben:** `COCKPIT_NOW=JJJJ-MM-TTTHH:MM` (nur Debug-Builds) ersetzt in „Heute“ und bei
  Strg+N die aktuelle Zeit, z. B. um den 2. GEC-Block zu testen:

  ```bash
  COCKPIT_NOW=2026-11-16T12:05 npm run tauri dev
  ```

  PowerShell: `$env:COCKPIT_NOW = "2026-11-16T12:05"; npm run tauri dev`.
- **Dev-Kopie in Obsidian öffnen:** Damit „In Obsidian öffnen“ funktioniert, muss Obsidian die Kopie als
  Vault kennen. Einmal in Obsidian *Vault wechseln → Ordner als Vault öffnen* und den Ordner aus der
  Zeile `[dev] Testvault-Kopie: …` wählen (sonst meldet Obsidian „Vault not found“). Nach einer neuen
  Kopie (anderer `<hash>`) das wiederholen. Obsidian legt dabei in der Kopie ein `.obsidian/` an –
  die Fixtures im Repository bleiben unberührt.
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
  write.rs                einziger Schreibweg: atomar, Konfliktschutz per SHA-256, nie überschreiben
  unterricht.rs           Unterrichtsnotiz aus der Vorlage, Fächer, Lernfelder
  heute.rs                Tagesansicht: Blöcke, Notizen, fällige Aufgaben
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
