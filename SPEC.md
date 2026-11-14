# Masterprompt — Ausbildungs-Cockpit

> **Wofür dieses Dokument ist:** Es ist der vollständige Auftrag an einen KI-Coding-Agenten
> (z. B. Claude Code), eine Desktop-App zu bauen. Es wird als Ganzes in die erste Nachricht
> eingefügt oder als `CLAUDE.md` / `SPEC.md` ins Repository gelegt. Alles, was der Agent wissen
> muss, steht hier — er hat keinen Zugriff auf das Gespräch, aus dem es entstanden ist.
>
> Stand: 02.11.2026 · Arbeitstitel „Ausbildungs-Cockpit" (änderbar)

> **Ergänzungen des Nutzers (09.–14.11.2026), gelten zusätzlich:**
> - Das Tool ist auch für **Mitschüler**: vollständiger Windows-Installer (NSIS), der auf Wunsch einen **leeren Starter-Vault** anlegt (`src-tauri/starter-vault/`, erzeugt mit `scripts/build-starter-vault.sh`, ohne persönliche Daten).
> - **Nur PC-Layout, nicht mobil.** Look nach zwei Referenz-Screenshots: dunkler Violett-Verlauf, halbtransparente Glas-Karten mit feinem hellem Rand und großen Radien, lila Balken mit Mint-Akzent, Pillen-Tabs, Mint/Rot-Badges für Veränderungen, große fette Zahlen, weiches Glühen. Navigation als linke Seitenleiste.
> - Auf dem PC des Hauptnutzers übernimmt die App die Daten aus dem bestehenden Vault (Fächer, Stundenplan, Templates) direkt aus den Dateien.
> - Repository: https://github.com/Ch3rry-glitch/Ausbildungs-Copilot

---

## 1 · Deine Rolle und der Auftrag

Du bist ein erfahrener Desktop-Entwickler mit Tauri, Rust und TypeScript. Du baust eine
Windows-Desktop-App, die einem Auszubildenden die tägliche Arbeit mit seinem **Obsidian-Lernvault**
abnimmt: Unterrichtsnotizen anlegen, Aufgaben und offene Fragen überblicken, das Berichtsheft
führen — und vor allem die **Logbucheinträge für Moodle automatisch erzeugen**.

Die App **ersetzt Obsidian nicht**. Sie ist eine zweite, schnellere Oberfläche auf **dieselben
Markdown-Dateien**. Der Vault bleibt die einzige Wahrheit. Obsidian muss jederzeit parallel
funktionieren, ohne dass irgendetwas kaputtgeht.

Arbeite in den Meilensteinen aus Abschnitt 11. Liefere jeden Meilenstein lauffähig ab, bevor du
den nächsten beginnst. Frag nach, wenn eine Anforderung hier mehrdeutig ist — rate nicht bei
allem, was Dateien im Vault verändert.

---

## 2 · Der Nutzer und sein Kontext

| | |
|---|---|
| **Ausbildung** | Kaufmann im E-Commerce, 1. Lehrjahr |
| **Schule** | Ludwig-Erhard-Berufskolleg Münster, Klasse **ECKU1**, Raum O131 |
| **Berufsschultage** | **Montag und Donnerstag** |
| **Betrieb** | Onlineshop (Name bewusst nicht im öffentlichen Repository) |
| **Lernplattform** | Moodle unter `https://www.lebk-moodle.de` |
| **Betriebssystem** | Windows |
| **Vault-Pfad** | z. B. `C:\Users\<Name>\Documents\E-Commerce` (in der App wählbar, nie fest verdrahtet) |

### Fächer, Lehrkräfte, Linkziele

Die Fachnotizen heißen im Vault **genau so**. Ein Link auf `[[WSP]]` zeigt ins Leere —
die App muss immer das vollständige Linkziel mit Alias schreiben.

| Kürzel | Fach | Lehrkraft | Linkziel im Vault |
|---|---|---|---|
| GEC | Geschäftsprozesse im E-Commerce | HEIN | `[[GEC – Geschäftsprozesse im E-Commerce\|GEC]]` |
| D-DV | Datenverarbeitung | HUGO | `[[D-DV]]` |
| WSP | Wirtschafts- und Sozialprozesse | SCMI | `[[WSP – Wirtschafts- und Sozialprozesse\|WSP]]` |
| KSK | Kaufmännische Steuerung und Kontrolle | BERN | `[[KSK – Kaufmännische Steuerung und Kontrolle\|KSK]]` |
| PG | Politik | Rothaus | `[[PG – Politik\|PG]]` |
| D | Deutsch | — | `[[D – Deutsch\|D]]` |
| E | Englisch | — | `[[E – Englisch\|E]]` |

### Stundenplan

Liegt im Vault unter `00 Start/Stundenplan.md` als Markdown-Tabellen. **Die App liest ihn von
dort**, statt ihn zu duplizieren. Stand heute:

| Std. | Zeit | Montag | Donnerstag |
|---:|---|---|---|
| 2 | 08:35–09:20 | GEC | KSK |
| 3 | 09:35–10:20 | D-DV | WSP |
| 4 | 10:20–11:05 | D-DV | E |
| 5 | 11:20–12:05 | WSP | GEC |
| 6 | 12:05–12:50 | GEC | GEC |
| 7 | 13:05–13:50 | GEC | PG |
| 8 | 13:50–14:35 | D | — |

### Lernfelder

`02 Lernfelder/LF01 – … .md` bis `LF12 – … .md`. Die vollständigen Dateinamen sind die Linkziele.
Die App liest sie aus dem Ordner, nicht aus einer festen Liste.

---

## 3 · Harte Regeln — nicht verhandelbar

Diese Regeln gehen **jeder** Funktion, jedem Komfort und jeder Annahme vor. Schreib für jede einen
automatisierten Test.

1. **Logbuch-Texte werden nie in den Vault geschrieben.** Das Logbuch lebt in Moodle. Die App
   erzeugt den Text, zeigt ihn an, kopiert ihn in die Zwischenablage — und speichert ihn **nirgends
   im Vault**. Erlaubt ist nur ein **Status-Feld** in der Frontmatter der Unterrichtsnotiz
   (`logbuch: offen | kopiert | abgegeben`), nie der Eintrag selbst.
2. **Die Selbstbewertung füllt nie die App aus.** Wo ein Fach eine Bewertung der Lerneinheit
   verlangt, bleibt das Feld leer (`__/10`), bis der Nutzer eine Zahl einträgt. Ohne Zahl kein
   „Kopieren" — stattdessen ein Hinweis.
3. **Krankheit im Ausbildungsnachweis nur als „arbeitsunfähig erkrankt".** Es gibt **kein Feld für
   eine Diagnose**, keinen Freitext dazu und keinen KI-Prompt, der danach fragt.
4. **Keine erfundenen Tätigkeiten im Ausbildungsnachweis.** Ein Nachweis wird unterschrieben.
   Stichpunkte ohne Substanz („Einschulung" ohne Gegenstand) werden **markiert**, nicht ausgeschmückt.
   Die KI darf formulieren, aber keinen Inhalt hinzufügen, der nicht in den Stichpunkten steht.
5. **Keine Passwörter.** Die App speichert keine Moodle-Zugangsdaten, gibt keine ein und loggt sich
   nirgends ein. Den Moodle-Login macht der Nutzer selbst im Browser.
6. **Interne Einkaufspreise (EK) verlassen die App nie** — nicht in kundenseitige Texte, nicht in
   KI-Prompts dafür. Erkennt die App Felder wie `ek`, `einkaufspreis` oder Formulierungen wie
   „EK:", werden sie vor jedem KI-Versand entfernt.
7. **Keine Behauptungen ohne Beleg.** Erzeugte Texte über Produkte oder Unterrichtsinhalte dürfen
   nur enthalten, was in der Quelle steht. Kein „Bio", wenn die Quelle „nachhaltig" sagt.
8. **`.obsidian/` wird nie angefasst** — weder gelesen noch geschrieben.
9. **Jede KI-erzeugte Änderung an einer bestehenden Datei zeigt vorher einen Diff** und braucht eine
   Bestätigung. Neue Dateien zeigen eine Vorschau.
10. **`10 Privat/` ist standardmäßig vom KI-Versand ausgeschlossen.** Einschalten nur pro Vorgang
    und ausdrücklich.

---

## 4 · Architektur

### Grundsatz: Der Vault ist die Datenbank

- Die App **liest und schreibt Markdown-Dateien** direkt im Vault-Ordner.
- Ein **Index** (Notizen, Frontmatter, Aufgaben, Tags, Links) wird beim Start aufgebaut und per
  Dateiwächter aktuell gehalten. Er darf in einer **SQLite-Datei im App-Datenordner**
  (`%APPDATA%\ausbildungs-cockpit\`) gecacht werden — er ist aber **jederzeit verwerfbar** und wird
  nie zur Quelle.
- App-Einstellungen (Vault-Pfad, Fächer-Konfiguration, Moodle-Adressen, KI-Modell) liegen im
  App-Datenordner, **nicht** im Vault.
- Der API-Schlüssel für die KI liegt im **Windows-Anmeldeinformationsspeicher** (Rust-Crate `keyring`),
  nie in einer Datei.

### Stack

| Schicht | Wahl | Begründung |
|---|---|---|
| Hülle | **Tauri 2** | kleiner Installer, schneller Start, sauberer Dateizugriff |
| Backend | **Rust** | Dateien, Wächter, Frontmatter, Index, KI-Aufrufe |
| Frontend | **React + TypeScript + Vite** | |
| Editor | **CodeMirror 6** mit Markdown-Modus | derselbe Unterbau wie Obsidian |
| Index-Cache | **SQLite** (`rusqlite`) | nur Cache |
| Dateiwächter | `notify` | erkennt Änderungen aus Obsidian |
| Frontmatter | `serde_yaml` mit **reihenfolge-erhaltendem** Mapping | siehe 5.2 |
| KI | **Anthropic Messages API** über `reqwest`, Modell in den Einstellungen wählbar | |
| Zwischenablage, URL öffnen | `tauri-plugin-clipboard-manager`, `tauri-plugin-opener` | |
| Paket | **NSIS**- oder MSI-Installer für Windows | |

Die KI-Anbindung steckt hinter einer Schnittstelle (`trait LlmProvider`), damit später ein anderes
Modell oder ein lokales möglich ist. **Ohne KI-Schlüssel muss die App vollständig benutzbar
bleiben** — Logbuch und Berichtsheft fallen dann auf eine deterministische Vorlagenbefüllung zurück.

### Verbindung zu Obsidian

- „In Obsidian öffnen" über `obsidian://open?vault=<Vaultname>&file=<relativer Pfad>`.
- Die App schreibt nur Formate, die Obsidian und seine Plugins (Dataview, Tasks) verstehen.
- **Dataview-Blöcke in Dateien bleiben unverändert.** Die App führt Dataview nicht aus, sondern
  bildet die wichtigsten Abfragen nativ nach (Abschnitt 6).

---

## 5 · Der Vault — so ist er gebaut

### 5.1 Ordner

```
00 Start/                Home, Dashboard, Aufgaben, Stundenplan, Zusammenarbeit mit Claude
01 Fächer/<Fach>/        eine Fachnotiz je Fach (+ Logbuch-Muster bei GEC)
02 Lernfelder/           LF01 … LF12
03 Unterricht/           eine Notiz je Unterrichtsstunde bzw. -block
04 Betrieb/              Betrieb.md, Projektnotizen
04 Betrieb/Wochen/       Berichtsheft je Kalenderwoche: 2026-KW41.md
05 Lernen/               Zusammenfassungen/, Karteikarten/, Prüfungsvorbereitung, Trainer
06 Glossar/              eine Notiz je Begriff
07 Material/_Inbox/      Eingang für Scans und Dateien, unsortiert
07 Material/Arbeitsblätter/   JJJJ-MM-TT Fach – Thema.pdf
07 Material/Altklausuren/
07 Material/Aufgabensammlung/ eine Notiz je Übungsaufgabe
10 Privat/               Reflexion/, Noten/, Lern-Log/   ← vom KI-Versand ausgeschlossen
99 Meta/Templates/       Altklausur, Berichtsheft-Woche, Lerneinheit, Logbuch-Eintrag GEC,
                         Material-Quelle, Prüfungsaufgabe, Unterrichtsnotiz, Zusammenfassung
```

**Die App benutzt die Templates aus `99 Meta/Templates/`**, nicht eigene Kopien. Ändert der Nutzer
ein Template in Obsidian, gilt das sofort auch in der App. Platzhalter wie `datum:` (leer) werden
befüllt, alles andere bleibt wörtlich.

### 5.2 Frontmatter — Regeln für das Schreiben

- **Unbekannte Schlüssel bleiben erhalten**, in ihrer Reihenfolge, mit ihrem Stil
  (`tags: [a, b]` bleibt Inline-Liste, `tags:\n  - a` bleibt Blockliste).
- Die App ändert nur die Schlüssel, die sie ändern soll. Ein Speichern ohne inhaltliche Änderung
  darf die Datei **byte-gleich** lassen.
- Datumsformat in der Frontmatter: `JJJJ-MM-TT`. Im Dateinamen: `TT.MM.JJJJ` (Unterricht) bzw.
  `JJJJ-MM-TT` (Material).

Häufigste Schlüssel im Bestand: `tags`, `fach`, `lernfeld`, `quelle`, `lehrkraft`, `typ`, `datum`,
`thema`, `schwierigkeit`, `geloest`, `status`, `kw`, `zeitraum`, `stunden`, `lernfelder`,
`abgezeichnet`, `klausur`, `kurz`.

### 5.3 Notiztypen und ihre Form

**Unterrichtsnotiz** — `03 Unterricht/<FACH> <TT.MM.JJJJ>.md`
- Zweiter Block desselben Fachs am selben Tag: `<FACH> 2 <TT.MM.JJJJ>.md`
- Altbestand hat andere Namen (`Unterrichtsnotiz GEC 07.09.2026.md`) — **lesen ja, umbenennen nein**.
- Frontmatter: `tags: [unterricht]`, `datum`, `fach`, `lernfeld`, `thema`, `lehrkraft`
- Überschrift: `# JJJJ-MM-TT FACH – Thema`
- Abschnitte aus dem Template: Status · Inhalt der Stunde · Wichtige Begriffe · Merksätze / Formeln ·
  Aufgaben / Hausaufgaben · Offene Fragen · Noch nicht verstanden · Verknüpft
- Neu durch die App, optional: `logbuch: offen | kopiert | abgegeben`

**Berichtsheft-Woche** — `04 Betrieb/Wochen/<JJJJ>-KW<nn>.md`
- Frontmatter: `tags: [betrieb, berichtsheft, unterricht]`, `kw`, `zeitraum`, `stunden`,
  `lernfelder: [2, 4]`, `status: In Arbeit | Abgeschlossen`, `abgezeichnet: true|false`
- Abschnitte: Berufsschule (je Schultag, je Fach eine Zeile mit Link) · Betrieb (je Betriebstag) ·
  Bezug zu Lernfeldern (Tabelle) · Ausformulierter Nachweis (Fließtext je Tag) · Offen vor dem Abzeichnen

**Aufgabe in der Aufgabensammlung** — `07 Material/Aufgabensammlung/<Titel>.md`
- Frontmatter: `tags: [aufgabe, lernaufgabe]`, `fach`, `lernfeld`, `typ`, `schwierigkeit`,
  `geloest: false`, `quelle: "[[Unterrichtsnotiz]]"`
- Lösung in einem zugeklappten Callout `> [!success]- Lösung`
- ⚠️ **Der Bestand ist uneinheitlich:** `typ` kommt als `rechnung` und als `Rechnen` vor,
  `schwierigkeit` als `mittel` und als `2`. Lesen: beides akzeptieren. Schreiben: die Werte aus
  `07 Material/Aufgabensammlung/Aufgabensammlung.md` (`rechnung · fallbeispiel · definition ·
  beurteilung · zuordnung`, `leicht · mittel · schwer`). Bestand **nicht** stillschweigend umschreiben —
  eine Bereinigung nur als eigene, bestätigte Aktion.

**Glossarbegriff** — `06 Glossar/<Begriff>.md`, Frontmatter `tags: [begriff]`, `kurz`, `lernfeld`, `fach`

### 5.4 Inline-Syntax, die die App versteht

| Syntax | Bedeutung |
|---|---|
| `- [ ] Text 📅 2026-11-05` | Aufgabe mit Fälligkeit (Tasks-Plugin-Format) |
| `- [x] …` | erledigt |
| `#frage` | offene Frage → Dashboard |
| `#nochmal` | nicht verstanden → Prüfungsvorbereitung |
| `[[Ziel\|Alias]]` | Wikilink |
| `> [!tip]`, `> [!warning]-` | Callouts, `-` = zugeklappt |

---

## 6 · Funktionen

### 6.1 Heute

Startbildschirm. Liest den Stundenplan und das Datum.

- **Schultag (Mo/Do):** Liste der heutigen Fächer in Reihenfolge. Je Fach: „Notiz anlegen" oder
  „Notiz öffnen", wenn es sie gibt. Zwei Blöcke desselben Fachs (z. B. GEC Std. 2 und 6–7) →
  Hinweis, ob eine oder zwei Notizen gewünscht sind.
- **Betriebstag:** Eingabefeld „Was hast du heute gemacht?" → Stichpunkt in die Wochennotiz.
- **Immer:** fällige Aufgaben (heute, überfällig, nächste 7 Tage), anstehende Klausuren,
  **offene Logbucheinträge** je Fach.

### 6.2 Unterrichtsnotiz

- Anlegen aus dem Template mit vorausgefülltem `datum`, `fach`, `lehrkraft` (aus dem Stundenplan),
  Überschrift und Fachlink. `lernfeld` und `thema` fragt die App ab.
- Bearbeiten im CodeMirror-Editor; Vorschau-Umschalter.
- **Stoff übernehmen:** Text einfügen, Datei oder Foto hineinziehen. Dann, wie bisher im Vault
  vereinbart:
  1. **Arbeitsaufträge herausziehen** → als `- [ ] … 📅 …` in „Aufgaben / Hausaufgaben"
  2. **Lernaufgaben erzeugen** → je Aufgabe eine Notiz in der Aufgabensammlung, mit Lösung
  3. **Begriffe vorschlagen** → Liste, auf Bestätigung als Glossarnotizen
  Alles mit Vorschau, nichts ohne Bestätigung.
- Schaltfläche **„Logbuch erzeugen"**, wenn für das Fach ein Logbuch aktiv ist.

### 6.3 Logbuch-Automatisierung — das Kernstück

**Ziel:** Aus den Unterrichtsnotizen eines Fachs und Datums entsteht ein fertiger Logbucheintrag,
der in Moodle eingefügt wird. Zwei Klicks: Kopieren, Moodle öffnen.

**Konfiguration je Fach** (in den Einstellungen, nicht im Vault):

| Feld | Beispiel GEC | Hinweis |
|---|---|---|
| Logbuch aktiv | ja | „für ausgewählte Fächer" — jedes Fach einzeln schaltbar |
| Moodle-Adresse | Kurs- oder Aktivitäts-URL | D-DV: `https://www.lebk-moodle.de/course/view.php?id=2644` |
| Abschnitte | siehe unten | frei definierbar, Reihenfolge änderbar |
| Selbstbewertung | Skala 1–10 | **nur GEC** verlangt sie bisher |
| Einträge pro Woche | 2 | GEC: Mo und Do |
| Umlaute | erhalten / umschreiben (ä → ae) | ältere GEC-Einträge wurden umgeschrieben; Standard: erhalten |
| Markdown entfernen | ja | Moodle bekommt reinen Text |

**Die GEC-Struktur** (von der Lehrkraft vorgegeben, Muster in `01 Fächer/GEC/Logbuch GEC.md` und
`99 Meta/Templates/Logbuch-Eintrag GEC.md`):

```
TT.MM.JJJJ

Dies habe ich erarbeitet:
- …

Diese Fragen haben sich ergeben:
- …

Das ist als naechstes zu tun:
- …

Bewertung dieser Lerneinheit: __/10
```

**Woher die Inhalte kommen:**

| Abschnitt | Quelle |
|---|---|
| Dies habe ich erarbeitet | „Inhalt der Stunde", „Merksätze", „Wichtige Begriffe" — **in ganzen Sätzen**, nicht in Stichworten, aus Ich-Sicht |
| Diese Fragen haben sich ergeben | `#frage`-Zeilen und „Noch nicht verstanden" — sind keine da, schlägt die KI **höchstens drei** vor und **kennzeichnet sie in der Vorschau als Vorschlag** |
| Das ist als nächstes zu tun | offene Aufgaben der Notiz, anstehende Termine (Klausuren) des Fachs |
| Bewertung | **leer, Eingabe durch den Nutzer** (Regel 2) |

**Ablauf in der App:**

1. Fach und Datum wählen (oder direkt aus der Notiz). Mehrere Notizen am selben Tag → zusammenführen.
2. Entwurf erzeugen (KI) oder Vorlage befüllen (ohne KI).
3. **Bearbeitbare Vorschau.** Vorgeschlagene Fragen markiert. Bewertungsfeld als Zahleneingabe.
4. **„Kopieren und Moodle öffnen"** → Text in die Zwischenablage, Moodle-Adresse des Fachs im
   Standardbrowser. Der Nutzer fügt ein und sendet ab.
5. Status in der Notiz-Frontmatter auf `logbuch: kopiert`. Der Nutzer kann auf `abgegeben` setzen.
6. Der **Eintragstext wird verworfen** — nicht im Vault, nicht im Index, nicht im Log (Regel 1).
   Nur ein Verlauf „Fach · Datum · Status · Zeitpunkt" im App-Datenordner ist erlaubt.

**Übersicht „Logbuch"**: je aktivem Fach eine Liste der Unterrichtstage mit Status
offen / kopiert / abgegeben, damit nichts liegen bleibt.

**Stil des erzeugten Textes** — so klangen die angenommenen Einträge:
- Ich-Form, Präteritum oder Perfekt, ganze Sätze
- je Punkt ein Gedanke; ein „Für mich mitgenommen: …" am Ende ist willkommen
- Fachbegriffe korrekt, keine Floskeln, keine Übertreibung
- nichts erfinden, was nicht in der Notiz steht (Regel 7)

### 6.4 Aufgaben

Nachbau der Seite `00 Start/Aufgaben.md`:
- **Arbeitsaufträge** (Pflicht, mit Frist) aus allen Unterrichtsnotizen
- **Lernaufgaben** aus der Aufgabensammlung (`geloest: false`)
- Filter nach Fach, Lernfeld, Fälligkeit. Abhaken in der App setzt `- [x]` bzw. `geloest: true`
  **in der Datei**.

### 6.5 Fragen und Lücken

- Alle `#frage`-Zeilen mit Quelle und Datum; „geklärt" entfernt das Tag nach Bestätigung
  (Zeile bleibt, Tag wird zu `~~#frage~~` oder in einen Abschnitt „Geklärt" verschoben — einstellbar).
- Alle `#nochmal`-Zeilen gruppiert nach Fach → Grundlage der Prüfungsvorbereitung.

### 6.6 Berichtsheft

- Wochennotiz aus `Berichtsheft-Woche.md` anlegen, KW und Zeitraum automatisch.
- Schultage füllen sich aus den Unterrichtsnotizen der Woche (Fach, Thema, Link).
- Betriebstage: Stichpunkte je Tag.
- **„Nachweis ausformulieren"**: Fließtext je Tag aus den Stichpunkten. Regeln 3 und 4 gelten
  hart: keine Diagnosen, keine erfundenen Tätigkeiten, dünne Stichpunkte werden als Lücke markiert.
- **Lernfeldbezug** je Tätigkeit vorschlagen → Tabelle + Frontmatter `lernfelder`.
- **`04 Betrieb/Betrieb.md` fortschreiben:** neue Überschneidungen Schule ↔ Betrieb in die
  vorhandene Tabelle — mit Diff und Bestätigung (Regel 9).
- Fehlende Tage und offene Punkte der Vorwochen sichtbar machen.

### 6.7 Inbox

- Dateien in `07 Material/_Inbox` erkennen (PDF, Bild, DOCX).
- Je Datei: Fach, Lernfeld, Datum, Thema vorschlagen (bei Scans per KI-Bilderkennung).
- Umbenennen nach `JJJJ-MM-TT Fach – Thema.ext`, verschieben nach `Arbeitsblätter` bzw.
  `Altklausuren`, Begleitnotiz aus dem passenden Template, Link in der Unterrichtsnotiz.
- Alles als Vorschau-Liste, ein Klick „Übernehmen".

### 6.8 Prüfungen

- Klausurtermine je Fach (aus Frontmatter `klausur:` der Notizen und aus der Prüfungsvorbereitung).
- Countdown auf „Heute".
- Je Klausur: verknüpfte Zusammenfassungen, offene `#nochmal`, ungelöste Lernaufgaben des Fachs.

---

## 7 · KI-Einsatz

- **Nur auf Auslösung durch den Nutzer**, nie im Hintergrund.
- Vor jedem Aufruf ist sichtbar, **welche Dateien** mitgeschickt werden. `10 Privat/` ist aus
  (Regel 10). Felder mit EK werden entfernt (Regel 6).
- Jeder Prompt liegt als eigene Datei im Repository (`prompts/logbuch.md`, `prompts/berichtsheft.md`,
  `prompts/lernaufgaben.md`, …), damit er ohne Neukompilieren angepasst werden kann.
- Jede Antwort wird **validiert**, bevor sie angezeigt wird: Struktur vorhanden? Bewertung leer?
  Keine Diagnose-Wörter im Berichtsheft? Bei Verstoß: verwerfen und mit Hinweis neu anfordern.
- Kosten und Tokenverbrauch je Aufruf in einem Verlauf im App-Datenordner.

---

## 8 · Oberfläche

- **Sprache: Deutsch.** Begriffe aus der Sicht des Nutzers: „Logbuch", „Berichtsheft",
  „Unterrichtsnotiz" — nicht „Entry", „Record", „Document".
- Linke Leiste: Heute · Unterricht · Logbuch · Aufgaben · Fragen · Berichtsheft · Inbox ·
  Prüfungen · Einstellungen.
- Hell und dunkel nach Windows-Einstellung.
- Tastaturkürzel: `Strg+N` neue Unterrichtsnotiz für die aktuelle Stunde, `Strg+L` Logbuch für die
  geöffnete Notiz, `Strg+K` Suche über den Vault.
- Jede schreibende Aktion endet mit einer Rückmeldung, was wohin geschrieben wurde —
  mit Link „In Obsidian öffnen".

---

## 9 · Sicherheit der Daten

- **Atomares Schreiben:** in eine temporäre Datei im selben Ordner, dann umbenennen.
- **Konfliktschutz:** Vor dem Schreiben Prüfsumme und Änderungszeit mit dem Stand beim Lesen
  vergleichen. Hat Obsidian die Datei inzwischen geändert → nicht überschreiben, Diff zeigen.
- **Sicherung vor Massenänderungen:** Betrifft eine Aktion mehr als eine Datei, vorher eine Kopie
  der betroffenen Dateien in `%APPDATA%\ausbildungs-cockpit\backups\<Zeitstempel>\`.
- Gesperrte Dateien (Cloud-Sync, Obsidian schreibt gerade) → wiederholen, nicht abbrechen.
- **Nie** löschen ohne Bestätigung. Verschieben statt löschen, wo möglich.

---

## 10 · Entwicklung und Tests

- **Entwickle nie gegen den echten Vault.** Lege unter `fixtures/vault/` einen Testvault mit derselben
  Ordnerstruktur und je zwei bis drei realistischen Beispielnotizen an — inklusive der Unsauberkeiten
  aus 5.3 (alte Dateinamen, uneinheitliche Frontmatter).
- **Unit-Tests (Rust):** Frontmatter-Rundlauf (lesen + unverändert schreiben = byte-gleich),
  Aufgaben-Parser, Tag-Parser, Dateinamens-Konvention, Stundenplan-Parser.
- **Regel-Tests:** je harter Regel aus Abschnitt 3 mindestens ein Test, der den Verstoß provoziert
  und das Blockieren prüft.
- **End-to-End:** Notiz anlegen → Stoff einfügen → Logbuch erzeugen → kopieren → Status `kopiert` →
  Vault enthält den Logbuchtext **nicht** (Volltextsuche über den Testvault).
- `README.md` mit Installation, Build (`cargo tauri build`), Einrichtung des API-Schlüssels.

---

## 11 · Meilensteine

Jeder Meilenstein endet mit einer lauffähigen App und einer kurzen Liste: was geht, was nicht,
was als Nächstes kommt.

| # | Inhalt | Fertig, wenn … |
|---|---|---|
| **M0** | Gerüst: Tauri-App, Vault wählen, Index lesend, Dateiwächter, Notizliste je Ordner, „In Obsidian öffnen" | Änderung in Obsidian erscheint in < 2 s in der App; nichts wird geschrieben |
| **M1** | Unterricht: Notiz aus Template, Editor, sicheres Speichern, „Heute"-Ansicht mit Stundenplan | Montag zeigt GEC, D-DV, WSP, GEC, D; neue Notiz ist in Obsidian sofort korrekt verlinkt |
| **M2** | **Logbuch**: Fach-Konfiguration, Erzeugen mit und ohne KI, Vorschau, Kopieren + Moodle öffnen, Status | GEC-Eintrag hat exakt die vier Abschnitte, Bewertung leer, Text **nicht** im Vault |
| **M3** | Aufgaben, Fragen, Lücken | Abhaken in der App ist in Obsidian sichtbar; Dataview-Seiten zeigen dasselbe |
| **M4** | Berichtsheft: Woche, Tage, Ausformulieren, Lernfeldbezug, Betrieb.md fortschreiben | Diagnose-Test und Erfindungs-Test schlagen an; Diff vor jeder Änderung an Betrieb.md |
| **M5** | Inbox und Stoffübernahme: einsortieren, Aufträge ziehen, Lernaufgaben, Glossar | drei Testscans landen korrekt benannt und verlinkt |
| **M6** | Prüfungen: Termine, Countdown, Lernstand je Klausur | Klausur erscheint auf „Heute" mit offenen `#nochmal` des Fachs |

**Beginne mit M0. Zeig mir vor dem Start von M1 die Ordnerstruktur des Repositories und die
Datenmodelle (Rust-Structs) — die Entscheidungen dort sind später am teuersten zu ändern.**

---

## 12 · Was ausdrücklich nicht gebaut wird

- kein Ersatz für Obsidian, kein eigener Graph, kein Plugin-System
- keine Cloud, kein Konto, kein Server — alles lokal
- **kein automatisches Absenden in Moodle**, kein Login, keine gespeicherten Zugangsdaten
- keine Speicherung von Logbuchtexten, an keinem Ort
- keine Bearbeitung von `.obsidian/`
- keine Umbenennung des Altbestands ohne ausdrücklichen Auftrag
