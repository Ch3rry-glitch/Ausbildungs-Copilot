---
tags: [start, moc, aufgaben]
---
# ✅ Aufgaben

Zwei Sorten, bewusst getrennt gehalten:

| | Woher | Verbindlich? | Wo sie liegen |
|---|---|---|---|
| **Arbeitsaufträge** | von der Lehrkraft vorgegeben | ja | als `- [ ]` in der jeweiligen Unterrichtsnotiz |
| **Lernaufgaben** | von Claude aus dem Stoff erzeugt | nein, zum Festigen | eigene Notizen in [[Aufgabensammlung]] |

Beides sammelt sich hier automatisch ein. Abgehakt wird dort, wo die Aufgabe steht — nicht auf dieser Seite.

---

## 🔴 Überfällig
```dataview
TASK
WHERE !completed AND due AND due < date(today)
  AND !contains(file.folder, "99 Meta")
  AND !startswith(file.name, "!")
SORT due ASC
```

## 📌 Diese Woche fällig
```dataview
TASK
WHERE !completed AND due
  AND due >= date(today) AND due <= date(today) + dur(7 days)
  AND !contains(file.folder, "99 Meta")
  AND !startswith(file.name, "!")
SORT due ASC
```

## 📋 Arbeitsaufträge ohne Frist
```dataview
TASK
WHERE !completed AND !due
  AND !contains(file.folder, "99 Meta")
  AND !startswith(file.name, "!")
  AND length(text) > 3
GROUP BY file.link
```

---

## 🧮 Lernaufgaben — noch offen
```dataview
TABLE WITHOUT ID
  file.link AS "Aufgabe",
  lernfeld AS "LF",
  fach AS "Fach",
  typ AS "Typ",
  schwierigkeit AS "Level"
FROM "07 Material/Aufgabensammlung"
WHERE file.name != "Aufgabensammlung" AND geloest != true
SORT lernfeld ASC, schwierigkeit ASC
```

## 🔁 Falsch gemacht — nochmal ran
```dataview
LIST
FROM #nochmal
WHERE !contains(file.folder, "99 Meta")
SORT file.mtime DESC
```

---

## 🆕 Eingang — noch nicht zugeordnet
Arbeitsaufträge, zu denen es noch keine Unterrichtsnotiz gibt.

- 

## Zuletzt erledigt
```dataview
TASK
WHERE completed AND !contains(file.folder, "99 Meta")
SORT file.mtime DESC
LIMIT 10
```
