// Linke Spalte: alle Wochen, neueste zuerst. Wochen ohne Notiz (Ferien?) sind gedämpft und
// lassen sich anlegen; Badges zeigen fehlende Tage, offene Punkte, Lücken und Gesundheitsangaben.
import { samePath, type BerichtsheftUebersicht, type WochenZeile } from "../../api";
import { Icon } from "../../icons";
import { anzahl, statusBadge, zeitraumKurz } from "./gemeinsam";

export function WochenListe({
  u,
  gewaehlt,
  onWaehlen,
  onAnlegen,
}: {
  u: BerichtsheftUebersicht;
  gewaehlt: string | null;
  onWaehlen: (path: string) => void;
  onAnlegen: (montag: string) => void;
}) {
  const jahrHeute = Number(u.heute.slice(0, 4));
  if (u.wochen.length === 0) {
    return <div className="hint pad">Noch keine Wochennotiz in „04 Betrieb/Wochen“.</div>;
  }
  return (
    <>
      {u.wochen.map((z) =>
        z.path ? (
          <WocheMitNotiz key={z.stem} z={z} jahrHeute={jahrHeute} aktiv={samePath(z.path, gewaehlt)} onClick={() => onWaehlen(z.path!)} />
        ) : (
          <div key={z.stem} className={`note-item bh-woche ohne ${z.aktuell ? "aktuell" : ""}`}>
            <div className="lb-day-row">
              <span className="lb-day-date">
                KW {z.kw}
                {z.jahr !== jahrHeute ? ` · ${z.jahr}` : ""}
              </span>
              <span className="bh-zeitraum">{zeitraumKurz(z.montag, z.sonntag)}</span>
            </div>
            <div className="note-meta">
              {z.aktuell && <span className="badge purple">diese Woche</span>}
              <span>keine Notiz (Ferien?)</span>
              <span className="filter-spacer" />
              <button className="btn small" onClick={() => onAnlegen(z.montag)} title={`Wochennotiz ${z.stem} anlegen`}>
                {Icon.plus} Anlegen
              </button>
            </div>
          </div>
        ),
      )}
    </>
  );
}

function WocheMitNotiz({ z, jahrHeute, aktiv, onClick }: { z: WochenZeile; jahrHeute: number; aktiv: boolean; onClick: () => void }) {
  const fehlen = z.fehlendeTage.length;
  return (
    <button className={`note-item bh-woche ${aktiv ? "active" : ""} ${z.aktuell ? "aktuell" : ""}`} onClick={onClick} title={z.path ?? undefined}>
      <div className="lb-day-row">
        <span className="lb-day-date">
          KW {z.kw}
          {z.jahr !== jahrHeute ? ` · ${z.jahr}` : ""}
          <span className="bh-zeitraum">{zeitraumKurz(z.montag, z.sonntag)}</span>
        </span>
        {z.status && <span className={`badge ${statusBadge(z.status)}`}>{z.status}</span>}
      </div>
      <div className="note-meta">
        {z.aktuell && <span className="badge purple">diese Woche</span>}
        {z.abgezeichnet && (
          <span className="badge mint">
            {Icon.check} abgezeichnet
          </span>
        )}
        {fehlen > 0 && (
          <span className="badge amber" title={`Ohne Inhalt: ${z.fehlendeTage.join(", ")}`}>
            {fehlen === 1 ? "1 Tag fehlt" : `${fehlen} Tage fehlen`}
          </span>
        )}
        {z.offenePunkte > 0 && (
          <span className="badge amber" title="Offen vor dem Abzeichnen">
            {z.offenePunkte} offen
          </span>
        )}
        {z.luecken > 0 && (
          <span className="badge amber" title="Stichpunkte ohne Gegenstand">
            {anzahl(z.luecken, "Lücke", "Lücken")}
          </span>
        )}
        {z.diagnosen > 0 && (
          <span className="badge red" title="Ein Stichpunkt nennt eine Gesundheitsangabe – bitte in Obsidian entfernen">
            Gesundheitsangabe
          </span>
        )}
        {z.stunden && <span>{z.stunden} Std.</span>}
      </div>
    </button>
  );
}
