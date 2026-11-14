---
tags: [fach, moc]
kuerzel: GEC
lehrkraft: HEIN
---
# Geschäftsprozesse im E-Commerce

## 📔 Logbuch (Pflicht)
Für dieses Fach führst du ein **Logbuch in Moodle**: nach jeder Stunde ein Eintrag mit Datum, *Dies habe ich erarbeitet*, *Diese Fragen haben sich ergeben*, *Das ist als nächstes zu tun* und deiner Selbstbewertung.

Das Ausbildungs-Cockpit erzeugt den Eintrag aus deiner Unterrichtsnotiz. Der Text wird **nicht** im Vault gespeichert, er gehört nach Moodle. Muster: [[Logbuch GEC]]

## Zugeordnete Lernfelder
- [[LF02 – Sortimente im Online-Vertrieb gestalten und die Beschaffung unterstützen]]
- [[LF03 – Verträge im Online-Vertrieb anbahnen und bearbeiten]]
- [[LF06 – Servicekommunikation kundenorientiert gestalten]]
- [[LF07 – Online-Marketing-Maßnahmen umsetzen und bewerten]]
- [[LF09 – Online-Vertriebskanäle auswählen]]

## Unterrichtsnotizen
```dataview
TABLE WITHOUT ID file.link AS "Stunde", lernfeld AS "LF", thema AS "Thema"
FROM "03 Unterricht"
WHERE fach = "GEC"
SORT file.name DESC
```

## Zusammenfassungen & Karteikarten
```dataview
LIST
FROM "05 Lernen"
WHERE fach = "GEC"
```

## Material & Altklausuren
```dataview
TABLE WITHOUT ID file.link AS "Material", typ AS "Typ", lernfeld AS "LF", datum AS "Datum"
FROM "07 Material"
WHERE fach = "GEC"
SORT datum DESC
```
