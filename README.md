# Ausbildungs-Cockpit

Windows-App für Auszubildende (Kaufmann/-frau im E-Commerce): eine zweite, schnellere Oberfläche auf
den eigenen **Obsidian-Lernvault**. Unterrichtsnotizen, Aufgaben, Berichtsheft und – das Kernstück –
Logbucheinträge für Moodle. Der Vault bleibt die einzige Wahrheit; Obsidian funktioniert parallel weiter.

> **Stand: M2 (Logbuch).** Startseite „Heute“ mit Stundenplan-Blöcken, fälligen Aufgaben und offenen
> Logbucheinträgen, Unterrichtsnotiz aus der Vorlage anlegen (Strg+N), Notizen im Editor bearbeiten und
> speichern (Strg+S) mit Konfliktschutz gegenüber Obsidian. **Logbuch:** Eintrag für Moodle aus den
> Unterrichtsnotizen eines Tages erzeugen (mit oder ohne KI), bearbeiten, Bewertung selbst eintragen,
> „Kopieren und Moodle öffnen“ (Strg+L aus der Notiz). Die App legt nur neue Notizen an, speichert geöffnete
> und setzt im Logbuch nur das Feld `logbuch: offen | kopiert | abgegeben` – sie löscht, verschiebt und
> benennt nichts um.

## Logbuch und KI-Schlüssel

- **Ohne KI-Schlüssel** funktioniert alles: „Ohne KI befüllen“ übernimmt Inhalt, Merksätze, Begriffe,
  `#frage`-Zeilen und offene Aufgaben wörtlich in die Abschnitte des Fachs.
- **Mit KI** (Anthropic): unter *Einstellungen → KI* einen API-Schlüssel (`sk-ant-…`, erstellt auf
  [platform.claude.com](https://platform.claude.com)) eintragen. Er liegt im
  **Windows-Anmeldeinformationsspeicher** (Eintrag `ausbildungs-cockpit`), nie in einer Datei, und wird nie
  wieder angezeigt. Die KI läuft nur auf Klick; vorher steht da, welche Notizen gesendet werden.
  EK-Angaben werden vorher entfernt, `10 Privat/` bleibt draußen (nur pro Vorgang einschaltbar).
  Kosten und Tokens je Aufruf stehen in `%APPDATA%\ausbildungs-cockpit\ki-verlauf.jsonl` (ohne Text).
- Der Prompt liegt in `prompts/logbuch.md` (im Programm eingebaut). Eine eigene Fassung unter
  `%APPDATA%\ausbildungs-cockpit\prompts\logbuch.md` hat Vorrang – ohne Neukompilieren.
- Welche Fächer ein Logbuch haben, ihre Moodle-Adresse, Abschnitte, Bewertungsskala und „Offen zählen ab“
  stehen unter *Einstellungen → Logbuch je Fach* (Voreinstellung: GEC mit Bewertung 1–10).
- Der Eintragstext wird **nirgends** gespeichert – nur in der Zwischenablage. Im App-Datenordner liegt
  lediglich ein Verlauf „Fach · Datum · Status · Zeitpunkt“ (`logbuch-verlauf.jsonl`).

## Aufgaben und Fragen (M3)

- **Aufgaben** rechnet wie die Dataview-Seite `00 Start/Aufgaben.md`: Quelle sind alle Notizen außer
  `99 Meta/` und Dateien, deren Name mit `!` beginnt (gilt auch für „Heute“). Listen: Überfällig ·
  Diese Woche fällig (heute bis heute + 7) · Später fällig (nur in der App) · Ohne Frist (Text länger als
  3 Zeichen, so fällt der Vorlagen-Platzhalter `- [ ]  📅 ` weg) · Zuletzt erledigt (10, neueste Notiz
  zuerst). `- [-]` (abgebrochen) zählt nicht als offen – Dataview zeigt sie als offen.
- **Abhaken** ändert in der Datei **nur das Zeichen zwischen `[` und `]`**: `- [ ]` → `- [x]` (zurück:
  `- [ ]`). Es wird kein `✅ Datum` angehängt; BOM und CRLF bleiben, alles andere ist byte-gleich. Steht
  die Aufgabe nicht mehr an ihrer Zeile (in Obsidian verschoben), sucht die App genau eine Aufgabe mit
  gleichem Text; bei keiner oder mehreren ändert sie nichts („bitte Liste neu laden“). Aufgaben in
  Codeblöcken sind keine Aufgaben.
- **Lernaufgaben** kommen aus `07 Material/Aufgabensammlung/` (auch Unterordner, ohne die Übersicht
  `Aufgabensammlung.md`). Gelöst ist nur `geloest: true` (YAML-Wahrheitswert, wie Dataview
  `geloest != true`). „Gelöst“ in der App ändert ausschließlich die Zeile `geloest:`; uneinheitlicher
  Bestand (`typ: Rechnen`, `schwierigkeit: 2`) bleibt unangetastet.
- **Fragen**: alle Zeilen mit `#frage` (auch `#Frage`, `#frage/…`, nicht `#fragebogen`), mit Quelle und
  Datum; **Nochmal**: alle `#nochmal`-Zeilen und Notizen mit `nochmal` in den Frontmatter-`tags`,
  gruppiert nach Fach. Leere Platzhalter aus der Vorlage (`- #frage `) erscheinen nicht.
- **„Geklärt“** zeigt vorher/nachher und schreibt erst nach Bestätigung. Einstellbar unter
  *Einstellungen → Fragen*:
  - *Tag durchstreichen* (Standard): die Zeile bleibt, wo sie ist; jedes `#frage` wird zu `~~#frage~~`
    (zählt dann für App und Dataview nicht mehr).
  - *In Abschnitt „Geklärt“ verschieben*: zusätzlich wandert die Zeile (ohne Einrückung) ans Ende des
    Abschnitts `## Geklärt` der Notiz (Überschrift beliebiger Ebene); fehlt er, legt die App ihn am
    Dateiende an.
- Gemischte Zeilenenden oder kaputte Frontmatter: die App ändert nichts und bittet, es in Obsidian zu tun.
  Notizen unter `10 Privat/` werden angezeigt (markiert); M3 benutzt keine KI.
- Code: `src-tauri/src/aufgaben.rs` (Listen, Abhaken, gelöst), `src-tauri/src/fragen.rs` (Fragen,
  Nochmal, geklärt), Tests in `src-tauri/tests/m3_aufgaben.rs`.

## Berichtsheft (M4)

- **Was die App schreibt:** neue Wochen `04 Betrieb/Wochen/<JJJJ>-KW<nn>.md` aus der Vorlage
  `99 Meta/Templates/Berichtsheft-Woche.md` (Vorschau vorher, nie überschreiben; nur `kw`, `zeitraum`,
  KW-/Datums-Platzhalter, Verknüpft-Links und die Fach-Zeilen der Schultage werden gefüllt). In
  bestehenden Wochen ändert sie nur die betroffenen Zeilen: Tag speichern, Schultage aus den
  Unterrichtsnotizen, Tagesblöcke im „Ausformulierten Nachweis“ (Callouts dahinter bleiben byte-gleich),
  Zeilen der Lernfeld-Tabelle + Frontmatter `lernfelder`. `04 Betrieb/Betrieb.md` bekommt nur neue
  Zeilen in der Tabelle „Tätigkeit … | Lernfeld …“.
- **Immer erst Vorschau/Diff (Regel 9):** jeder schreibende Befehl liefert mit `vorschau: true` den
  ganzen neuen Text; geschrieben wird nur mit der `basis` aus dieser Vorschau – hat Obsidian die Datei
  inzwischen geändert, schreibt die App nichts („bitte Vorschau neu laden“).
- **Regel 3:** Krankheit ist nur die Tagesart „Arbeitsunfähig erkrankt“ – kein Feld, kein Freitext, kein
  Prompt für eine Diagnose; Stichpunkte, Nachweis-Texte und KI-Antworten mit Diagnose-Wörtern
  (`berichtsheft::diagnose_wort`) werden abgelehnt, Krank-Tage gehen nie an die KI.
  **Regel 4:** dünne Stichpunkte („Einschulung“) werden als `⟨Lücke: „…“ – Gegenstand fehlt⟩` markiert,
  nicht ausformuliert; KI-Antworten mit Sätzen ohne Beleg, fremden Zahlen oder nicht markierten Lücken
  werden verworfen (einmal neu angefordert, dann Fehler).
- **KI** nur auf Klick, `10 Privat/` nur pro Vorgang, EK wird vorher entfernt. Prompts:
  `prompts/berichtsheft.md` (Nachweis) und `prompts/berichtsheft-lernfelder.md` (Lernfeldbezug) – eigene
  Fassungen unter `%APPDATA%\ausbildungs-cockpit\prompts\` haben Vorrang.
- Code: `src-tauri/src/berichtsheft/` (`woche`, `anlegen`, `tag`, `nachweis`, `lernfelder`, `betrieb`,
  `regeln`), Tests in `src-tauri/tests/m4_berichtsheft.rs`.

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
  logbuch.rs              Logbuch: Inhalt aus der Notiz, Text für Moodle, Status `logbuch:`, Übersicht
  ki.rs                   KI hinter `LlmProvider` (Anthropic), Schlüssel via keyring, Antwort prüfen
  starter.rs              Starter-Vault anlegen, nie überschreiben
  obsidian.rs             obsidian://-Links
  config.rs               Einstellungen in %APPDATA%\ausbildungs-cockpit
  commands.rs             Tauri-Befehle
prompts/                  KI-Prompts (ohne Neukompilieren anpassbar)
src-tauri/tests/          Tests gegen fixtures/vault
fixtures/vault/           Testvault mit absichtlich unsauberem Altbestand
```

## Harte Regeln (Auszug)

Logbuch-Texte landen nie im Vault · keine Selbstbewertung durch die App · keine Diagnosen im
Berichtsheft · keine erfundenen Tätigkeiten · keine Passwörter · EK-Preise verlassen die App nie ·
`.obsidian/` wird nie angefasst · `10 Privat/` ist vom KI-Versand ausgeschlossen.
