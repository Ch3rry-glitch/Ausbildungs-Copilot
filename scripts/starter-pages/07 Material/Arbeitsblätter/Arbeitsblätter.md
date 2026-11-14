---
tags: [moc, material]
---
# 📄 Arbeitsblätter

Dateiname `JJJJ-MM-TT Fach – Thema.pdf`. Neue Scans zuerst nach `07 Material/_Inbox`.

```dataview
TABLE WITHOUT ID file.link AS "Blatt", fach AS "Fach", lernfeld AS "LF", datum AS "Datum"
FROM "07 Material/Arbeitsblätter"
WHERE file.name != this.file.name
SORT datum DESC
```
