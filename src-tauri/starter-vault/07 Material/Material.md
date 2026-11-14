---
tags: [moc, material]
---
# 📂 Material

Alles, was **von außen** in den Vault kommt: gescannte Altklausuren, Arbeitsblätter, Skripte, Aufgabensammlungen. Eigene Texte gehören nicht hierher — die liegen in [[Zusammenfassungen]], [[Karteikarten]] und den Lernfeldern.

## Bereiche
- 📥 [[_Inbox]] — **hier landet alles Neue.** Scan rein, Rest später.
- 📝 [[Altklausuren]] — alte Klausuren und Prüfungen, nach Lernfeld sortiert
- 📄 [[Arbeitsblätter]] — Handouts und Arbeitsblätter aus dem Unterricht
- 🧮 [[Aufgabensammlung]] — einzelne Aufgaben zum Üben, aus Klausuren herausgelöst

## So kommt ein Scan in den Vault
1. Datei (PDF/Bild) nach `07 Material/_Inbox/` legen — Name egal.
2. Claude sagen: *„Verarbeite die Inbox."*
3. Claude legt dazu eine Notiz an (Template *Altklausur* oder *Material-Quelle*), trägt Fach, Lernfeld, Jahr und Themen ein, verschiebt die Datei in den passenden Ordner und verlinkt sie im Lernfeld.

> Die Original-Datei bleibt immer erhalten und wird in der Notiz mit `![[Dateiname.pdf]]` eingebettet.

## Alles Material auf einen Blick
```dataview
TABLE WITHOUT ID
  file.link AS "Notiz",
  typ AS "Typ",
  fach AS "Fach",
  lernfeld AS "LF",
  jahr AS "Jahr"
FROM "07 Material"
WHERE typ
SORT jahr DESC, file.name ASC
```

## Noch unverarbeitet
```dataview
LIST
FROM "07 Material/_Inbox"
WHERE file.name != "_Inbox"
```
