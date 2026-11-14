---
tags: [moc, start]
---
# 🏠 Home – Ausbildung E-Commerce

> Einstiegspunkt für den Vault. Von hier aus erreichst du alles.

## Schnellzugriff
- 📊 [[Dashboard]] – was diese Woche ansteht
- ✅ [[Aufgaben]] – Arbeitsaufträge und Lernaufgaben
- 📅 [[Stundenplan]]
- 🗺️ [[Ausbildungsplan]] – alle 12 Lernfelder auf einen Blick
- 🎓 [[Prüfungen]]
- 🧠 [[Prüfungsvorbereitung]]
- 📂 [[Material]] – Altklausuren, Arbeitsblätter, Scans
- 📖 [[Glossar]]
- 🔒 [[Privat]] – Noten, Lern-Log, Reflexion
- 🤖 [[Zusammenarbeit mit Claude]]

## Fächer
- [[GEC – Geschäftsprozesse im E-Commerce]]
- [[WSP – Wirtschafts- und Sozialprozesse]]
- [[KSK – Kaufmännische Steuerung und Kontrolle]]
- [[D-DV]] · [[E – Englisch]] · [[PG – Politik]] · [[D – Deutsch]]

## Aktueller Stand der Lernfelder
```dataview
TABLE WITHOUT ID
  file.link AS "Lernfeld",
  fach AS "Fach",
  jahr AS "Jahr",
  status AS "Status"
FROM "02 Lernfelder"
SORT file.name ASC
```

## Offene Aufgaben (aus allen Notizen)
```dataview
TASK
WHERE !completed
SORT due ASC
LIMIT 15
```

## Letzte Unterrichtsnotizen
```dataview
LIST
FROM "03 Unterricht"
WHERE file.name != "Unterricht"
SORT file.mtime DESC
LIMIT 8
```

## Zuletzt hinzugekommenes Material
```dataview
TABLE WITHOUT ID file.link AS "Material", typ AS "Typ", fach AS "Fach", lernfeld AS "LF"
FROM "07 Material"
WHERE typ
SORT file.mtime DESC
LIMIT 8
```

## Offene Fragen (überall im Vault mit `#frage` markiert)
```dataview
LIST
FROM #frage
SORT file.mtime DESC
LIMIT 15
```
