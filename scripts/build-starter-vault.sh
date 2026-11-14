#!/usr/bin/env bash
# Erzeugt src-tauri/starter-vault/ – den leeren Vault, den der Installer für
# Mitschüler mitbringt. Quelle ist ein bestehender Vault (nur gelesen).
#
#   bash scripts/build-starter-vault.sh "/c/Users/<name>/Documents/E-Commerce"
#
# Übernommen werden nur allgemeine Seiten (Templates, Lernfelder, Übersichten).
# Persönliches (Unterrichtsnotizen, Berichtsheft, Betrieb, 10 Privat, Glossar-
# Einträge, Material, eigene Notizen in Lernfeldern) wird NICHT kopiert.
# .obsidian/ wird weder gelesen noch kopiert (Regel 8).
set -euo pipefail

SRC="${1:?Pfad zum Quell-Vault angeben}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
S="$ROOT/src-tauri/starter-vault"

rm -rf "$S"
mkdir -p "$S"
cd "$S"

mkdir -p "00 Start" "02 Lernfelder" "03 Unterricht" "04 Betrieb/Wochen" \
  "05 Lernen/Zusammenfassungen" "05 Lernen/Karteikarten" "06 Glossar" \
  "07 Material/_Inbox" "07 Material/Arbeitsblätter" "07 Material/Altklausuren" \
  "07 Material/Aufgabensammlung" "10 Privat/Reflexion" "10 Privat/Noten" \
  "10 Privat/Lern-Log" "99 Meta/Templates" \
  "01 Fächer/GEC" "01 Fächer/D-DV" "01 Fächer/WSP" "01 Fächer/Ksk" \
  "01 Fächer/Politik" "01 Fächer/Deutsch" "01 Fächer/Englisch"

# ---------------------------------------------------------------- Templates
cp "$SRC/99 Meta/Templates/"*.md "99 Meta/Templates/"

# ---------------------------------------------------------------- Lernfelder
# Alles bis "## Meine Notizen" übernehmen, Haken zurücksetzen, Status "offen".
for f in "$SRC/02 Lernfelder/"LF*.md; do
  b="$(basename "$f")"
  awk '/^## Meine Notizen/{print; print "-"; exit} {print}' "$f" \
    | sed -e 's/^- \[x\]/- [ ]/' -e 's/^status: .*/status: offen/' \
          -e 's/\*\*Status:\*\* `[^`]*`/**Status:** `offen`/' > "02 Lernfelder/$b"
done

# ---------------------------------------------------------------- allgemeine Seiten
copy_if() { [ -f "$SRC/$1" ] && cp "$SRC/$1" "$1" || true; }
copy_if "00 Start/Dashboard.md"
copy_if "00 Start/Aufgaben.md"
copy_if "00 Start/Ausbildungsplan.md"
copy_if "00 Start/Prüfungen.md"
copy_if "00 Start/Zusammenarbeit mit Claude.md"
copy_if "00 Start/Home.md"
copy_if "07 Material/Material.md"
copy_if "07 Material/Aufgabensammlung/Aufgabensammlung.md"
copy_if "05 Lernen/Karteikarten/Karteikarten.md"

# Aufgaben-Eingang leeren (dort können persönliche Aufträge stehen)
if [ -f "00 Start/Aufgaben.md" ]; then
  awk '/^## 🆕 Eingang/{print; skip=1; next}
       skip && /^## /{skip=0; print "Arbeitsaufträge, zu denen es noch keine Unterrichtsnotiz gibt.\n\n- \n"}
       !skip{print}' "00 Start/Aufgaben.md" > tmp && mv tmp "00 Start/Aufgaben.md"
fi
# Verweis auf eine Seite, die es im Starter-Vault nicht gibt
sed -i 's/ — siehe \[\[Relay-Freigaben\]\]//' "00 Start/Zusammenarbeit mit Claude.md" 2>/dev/null || true

# ---------------------------------------------------------------- generierte Seiten
cp -r "$ROOT/scripts/starter-pages/." .

echo "Starter-Vault erzeugt in $S"
# Kontrolle: keine persönlichen Spuren.
# Allgemein: keine Benutzerpfade, keine E-Mail-Adressen. Zusätzlich eigene Wörter (Name, Betrieb …)
# über die Umgebungsvariable STARTER_FORBIDDEN_WORDS="wort1,wort2" – stehen so nicht im Repository.
PATTERNS=(-e '[A-Za-z]:[\\/]Users[\\/]' -e '/c/Users/' -e '[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}')
IFS=',' read -ra EXTRA <<< "${STARTER_FORBIDDEN_WORDS:-}"
for w in "${EXTRA[@]}"; do [ -n "$w" ] && PATTERNS+=(-e "$w"); done
if grep -rIlE -i "${PATTERNS[@]}" . ; then
  echo "WARNUNG: persönliche Daten im Starter-Vault gefunden (siehe oben)" >&2
  exit 1
fi
