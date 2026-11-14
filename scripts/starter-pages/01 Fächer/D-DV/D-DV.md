---
tags: [fach, moc]
kuerzel: D-DV
lehrkraft: HUGO
---
# Datenverarbeitung

## Zugeordnete Lernfelder
- *(kein Lernfeld zugeordnet – berufsübergreifendes Fach)*

## Unterrichtsnotizen
```dataview
TABLE WITHOUT ID file.link AS "Stunde", lernfeld AS "LF", thema AS "Thema"
FROM "03 Unterricht"
WHERE fach = "D-DV"
SORT file.name DESC
```

## Zusammenfassungen & Karteikarten
```dataview
LIST
FROM "05 Lernen"
WHERE fach = "D-DV"
```

## Material & Altklausuren
```dataview
TABLE WITHOUT ID file.link AS "Material", typ AS "Typ", lernfeld AS "LF", datum AS "Datum"
FROM "07 Material"
WHERE fach = "D-DV"
SORT datum DESC
```
