---
tags: [moc, betrieb, berichtsheft]
---
# 📒 Berichtsheft

Eine Notiz je Kalenderwoche in `04 Betrieb/Wochen/`, Dateiname `JJJJ-KWnn`, angelegt aus dem Template *Berichtsheft-Woche*.

```dataview
TABLE WITHOUT ID file.link AS "Woche", zeitraum AS "Zeitraum", status AS "Status", abgezeichnet AS "Abgezeichnet"
FROM "04 Betrieb/Wochen"
SORT file.name DESC
```

> [!warning] Krankheit
> Im Nachweis steht nur „arbeitsunfähig erkrankt". Keine Diagnose, keine Details.
