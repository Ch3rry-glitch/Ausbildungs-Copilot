---
tags: [moc]
---
# 🃏 Karteikarten

Ein Set pro Lernfeld/Thema (Template *Karteikarten-Set*). Format je Karte:

```
**Frage?**
?
Antwort
```

Das Format ist kompatibel mit dem Plugin **Spaced Repetition** (Frage, Zeile mit `?`, Antwort) – optional installierbar, dann kann Obsidian dich abfragen.

```dataview
TABLE WITHOUT ID file.link AS "Set", fach AS "Fach", lernfeld AS "LF"
FROM "05 Lernen/Karteikarten"
WHERE file.name != this.file.name
SORT lernfeld ASC
```
