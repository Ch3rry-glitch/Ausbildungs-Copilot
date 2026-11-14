---
tags: [moc, material, aufgaben]
---
# 🧮 Aufgabensammlung

Einzelne Aufgaben zum Üben — aus Altklausuren herausgelöst oder von Claude im IHK-Stil erzeugt. Eine Notiz pro Aufgabe, damit sie sich nach Lernfeld und Schwierigkeit filtern lassen.

Jede Aufgabe hat im Frontmatter:

| Feld | Werte |
|------|-------|
| `lernfeld` | 1–12 |
| `fach` | GEC · WSP · KSK · D · E · PG · D-DV |
| `typ` | rechnung · fallbeispiel · definition · beurteilung · zuordnung |
| `schwierigkeit` | leicht · mittel · schwer |
| `quelle` | Altklausur-Link oder „generiert" |
| `geloest` | true / false |

## Alle Aufgaben
```dataview
TABLE WITHOUT ID
  file.link AS "Aufgabe",
  lernfeld AS "LF",
  typ AS "Typ",
  schwierigkeit AS "Level",
  geloest AS "Gelöst"
FROM "07 Material/Aufgabensammlung"
WHERE file.name != this.file.name
SORT lernfeld ASC
```

## Noch offen — hier üben
```dataview
TABLE WITHOUT ID file.link AS "Aufgabe", lernfeld AS "LF", typ AS "Typ"
FROM "07 Material/Aufgabensammlung"
WHERE file.name != this.file.name AND geloest != true
SORT lernfeld ASC
LIMIT 20
```

## Falsch gemacht — nochmal ansehen
Aufgaben, die mit `#nochmal` markiert sind:
```dataview
LIST
FROM #nochmal
SORT file.mtime DESC
```
