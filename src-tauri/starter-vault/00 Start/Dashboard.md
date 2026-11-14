---
tags:
  - start
  - dashboard
---
# 📊 Dashboard

Die Woche auf einen Blick. Für den Einstieg in den Vault siehe [[Home]], für alles Abzuarbeitende [[Aufgaben]].

## ⚠️ Was jetzt ansteht
```dataview
TASK
WHERE !completed AND due AND due <= date(today) + dur(14 days)
SORT due ASC
LIMIT 20
```

## 📥 Inbox — unverarbeitetes Material
```dataview
LIST
FROM "07 Material/_Inbox"
WHERE file.name != "_Inbox"
SORT file.mtime ASC
```
> Leer? Gut. Nicht leer? *„Verarbeite die Inbox."*

## 📝 Altklausuren ohne Auswertung
```dataview
TABLE WITHOUT ID file.link AS "Klausur", fach AS "Fach", lernfeld AS "LF"
FROM "07 Material/Altklausuren"
WHERE file.name != "Altklausuren" AND ausgewertet != true
```

## 📓 Berichtsheft offen
```dataview
TABLE WITHOUT ID file.link AS "Woche", zeitraum AS "Zeitraum", status AS "Status"
FROM "04 Betrieb/Wochen"
WHERE status != "fertig" AND status != "Abgeschlossen"
SORT file.name DESC
LIMIT 5
```

## 🎯 Lernfelder in Arbeit
```dataview
TABLE WITHOUT ID file.link AS "Lernfeld", fach AS "Fach", status AS "Status"
FROM "02 Lernfelder"
WHERE status = "in-arbeit"
SORT file.name ASC
```

## 🕐 Zuletzt bearbeitet
```dataview
TABLE WITHOUT ID file.link AS "Notiz", file.folder AS "Ordner", file.mtime AS "Geändert"
FROM "" 
WHERE file.name != this.file.name AND !contains(file.folder, "99 Meta")
SORT file.mtime DESC
LIMIT 10
```

## ❓ Offene Fragen
```dataview
LIST
FROM #frage
SORT file.mtime DESC
LIMIT 15
```

## 🔁 Nochmal ansehen
Alles, was als noch nicht sitzend markiert ist:
```dataview
LIST
FROM #nochmal
SORT file.mtime DESC
LIMIT 15
```

## 🧭 Verwaiste Notizen
Notizen, auf die nichts verlinkt — meist ein Zeichen, dass eine Verknüpfung fehlt:
```dataview
LIST
FROM "" 
WHERE length(file.inlinks) = 0 AND !contains(file.folder, "99 Meta") AND !contains(file.name, "Inbox")
SORT file.folder ASC
LIMIT 20
```
