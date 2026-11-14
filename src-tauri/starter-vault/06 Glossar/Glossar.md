---
tags: [moc, glossar]
---
# 📖 Glossar

Eine Notiz je Begriff, angelegt aus dem Template *Begriff*.

```dataview
TABLE WITHOUT ID file.link AS "Begriff", kurz AS "Kurz", fach AS "Fach"
FROM "06 Glossar"
WHERE file.name != this.file.name
SORT file.name ASC
```
