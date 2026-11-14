---
tags: [meta]
---
# 🤖 Zusammenarbeit mit Claude (Cowork)

So ist der Vault gedacht, damit Claude dir beim Lernen helfen kann.

## Konventionen
- **Jede Unterrichtsstunde** → eine Notiz in `03 Unterricht` (Template *Unterrichtsnotiz*). Im Frontmatter `fach` und `lernfeld` eintragen, dann tauchen die Notizen automatisch in den Fach- und Lernfeld-Übersichten auf.
- **Begriffe** → eine Notiz pro Begriff in `06 Glossar` (Template *Begriff*). Im Fließtext einfach `[[Begriff]]` verlinken.
- **Material von außen** → nach `07 Material/_Inbox`. Siehe unten.
- **Offene Fragen** im Text mit `#frage` markieren → erscheinen auf [[Home]] und im [[Dashboard]].
- **Was nicht sitzt** mit `#nochmal` markieren → landet in der [[Prüfungsvorbereitung]].
- **Aufgaben** als `- [ ] Aufgabe 📅 2026-09-10` schreiben.
- **Status** eines Lernfelds im Frontmatter pflegen: `offen` → `in-arbeit` → `abgeschlossen`.
- **Privates** (Noten, Berichtsheft, Mitschriften) bleibt in `03`, `04` und `10 Privat`.

## 📥 Scans und Altklausuren füttern
Der wichtigste Arbeitsablauf im Vault:

1. Klausur/Arbeitsblatt scannen oder abfotografieren.
2. Datei nach `07 Material/_Inbox` legen — Name egal, nicht sortieren.
3. Claude sagen: **„Verarbeite die Inbox."**

Claude liest die Dateien (auch Scans per OCR), erkennt Fach und Lernfeld, legt eine Notiz aus dem Template *Altklausur* bzw. *Material-Quelle* an, benennt die Datei nach `JJJJ-MM-TT Fach – Thema` um, verschiebt sie in den richtigen Ordner und verlinkt sie im Lernfeld.

Danach lohnen sich diese Folgeaufträge:
> „Zieh aus der Klausur die Einzelaufgaben in die Aufgabensammlung."
> „Mach mir Karteikarten aus den Themen dieser Klausur."
> „Welche Themen kommen in allen Altklausuren zu GEC vor?"

## 📚 Unterrichtsstoff geben — was automatisch passiert
Sobald du Stoff lieferst (Tafelfoto, Arbeitsblatt, Mitschrift, Folien, auch nur hingetippt), macht Claude ohne weitere Ansage drei Dinge:

1. **Arbeitsaufträge herausziehen** — alles, was die Lehrkraft aufgegeben hat, wird als `- [ ] Auftrag 📅 JJJJ-MM-TT` in die zugehörige Unterrichtsnotiz geschrieben. Von dort sammelt [[Aufgaben]] es automatisch ein. Gibt es noch keine passende Notiz, landet der Auftrag im Abschnitt *Eingang* auf der Aufgaben-Seite.
2. **Lernaufgaben erzeugen** — zusätzlich baut Claude eigene Übungsaufgaben zum Stoff (Template *Prüfungsaufgabe*) und legt sie in [[Aufgabensammlung]] ab: eine Notiz pro Aufgabe, mit Lösung zum Aufklappen, Schwierigkeitsgrad und Lernfeld.
3. **Begriffe vorschlagen** — neue Fachbegriffe werden gesammelt und auf Zuruf als Notizen im [[Glossar]] angelegt.

Der Unterschied zählt: Arbeitsaufträge sind Pflicht und haben eine Frist, Lernaufgaben sind zum Festigen und dürfen liegen bleiben. Beide stehen getrennt auf [[Aufgaben]].

## 📔 GEC-Logbuch — nur zum Kopieren, nicht in den Vault
Für GEC ist ein Logbuch in Moodle Pflicht. Kommt GEC-Stoff in den Vault, liefert Claude den fertigen Eintrag **im Chat** — in der von HEIN vorgegebenen Struktur:

- **Dies habe ich erarbeitet** — aus dem Inhalt der Stunde, in ganzen Sätzen statt Stichworten
- **Diese Fragen haben sich ergeben** — aus `#frage`-Markierungen und offen gebliebenen Punkten
- **Das ist als nächstes zu tun** — aus Hausaufgaben, eigenen Vorhaben und anstehenden Terminen
- **Bewertung der Lerneinheit** — Skala 1–10, die setzt du selbst

> [!important] Der Eintrag wird **nicht** in den Vault geschrieben
> Das Logbuch lebt in Moodle. Claude gibt den Text als Codeblock im Chat aus, du kopierst ihn direkt hinüber. [[Logbuch GEC]] enthält nur noch die Struktur und die älteren Einträge als Beispiel — dort kommt nichts Neues mehr dazu.

## 🏢 Berichtsheft ausformulieren — was automatisch mitläuft
Beim Auftrag *„Formuliere das Berichtsheft für KW nn aus."* passiert zusätzlich zum Fließtext:

1. **Lernfeldbezug bestimmen** — jede Tätigkeit wird einem Lernfeld zugeordnet, in der Wochennotiz als Tabelle und im Frontmatter als `lernfelder: [2, 5, 8]`.
2. **[[Betrieb]] fortschreiben** — neue Überschneidungen zwischen Betrieb und Schule wandern in die Tabelle *Praxis ↔ Schule*. Die Abdeckungsübersicht dort zeigt danach, welche Lernfelder im Betrieb noch nie vorkamen.
3. **Lücken benennen** — Stichpunkte ohne Substanz („Einschulung" ohne Gegenstand) werden im Text markiert statt stillschweigend ausgeschmückt. Ein Ausbildungsnachweis wird unterschrieben; erfundene Tätigkeiten haben darin nichts verloren.

## Was Claude für dich tun kann
- **Inbox verarbeiten** — Scans einsortieren, verschlagworten, verlinken
- **Altklausuren auswerten** — Themenhäufigkeit, Aufgabentypen, Prognose für die nächste Klausur
- **Übungsaufgaben** im IHK-Stil erzeugen, mit Lösungsweg
- **Karteikarten** (Frage/Antwort) aus Notizen und Klausuren → `05 Lernen/Karteikarten`
- **Zusammenfassungen** je Lernfeld → `05 Lernen/Zusammenfassungen`
- **Lernplan** vor Klausuren und Prüfungen, abgestimmt auf schwache Themen
- **Berichtsheft** aus Stichpunkten ausformulieren → `04 Betrieb/Wochen`
- **Vault aufräumen** — fehlende Verlinkungen, Dubletten im Glossar, verwaiste Notizen

## Typische Aufträge
> „Verarbeite die Inbox."
> „Sieh dir die Unterrichtsnotizen der letzten zwei Wochen an und erstelle daraus Karteikarten für LF03."
> „Erstelle mir eine Zusammenfassung zu LF04 auf Basis meiner Notizen und der Altklausuren."
> „Stell mir 10 Prüfungsfragen zu Online-Marketing, Schwierigkeit mittel."
> „Formuliere das Berichtsheft für KW 37 aus."
> „Prüfe den Vault auf fehlende Verlinkungen und ergänze sie."
> „Was sollte ich diese Woche lernen?"

## Damit Claude gut arbeiten kann
- Frontmatter ausfüllen, auch wenn es lästig ist — davon lebt jede Übersicht.
- Lieber viele kleine Notizen als eine große.
- Rohmaterial (Scans) nie löschen: Claude arbeitet mit dem Original weiter.
