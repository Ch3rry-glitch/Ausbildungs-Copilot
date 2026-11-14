---
tags: [moc, material, altklausur]
---
# 📝 Altklausuren

```dataview
TABLE WITHOUT ID file.link AS "Klausur", fach AS "Fach", datum AS "Datum", ausgewertet AS "Ausgewertet"
FROM "07 Material/Altklausuren"
WHERE file.name != this.file.name
SORT datum DESC
```
