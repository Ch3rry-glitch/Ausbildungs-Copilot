---
tags: [moc, unterricht]
---
# 🏫 Unterricht

Eine Notiz je Unterrichtsstunde bzw. -block. Dateiname `FACH TT.MM.JJJJ`, zweiter Block am selben Tag `FACH 2 TT.MM.JJJJ`.

```dataview
TABLE WITHOUT ID file.link AS "Stunde", fach AS "Fach", lernfeld AS "LF", thema AS "Thema"
FROM "03 Unterricht"
WHERE file.name != this.file.name
SORT datum DESC
```
