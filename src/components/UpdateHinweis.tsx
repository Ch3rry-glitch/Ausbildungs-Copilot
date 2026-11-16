// Kleiner Hinweis in der Seitenleiste, wenn eine neue Version bereitliegt.
import { useEffect } from "react";
import { Icon } from "../icons";
import { beimStartPruefen, installieren, spaeter, useUpdate } from "../update";

export function UpdateHinweis({ version }: { version: string }) {
  const u = useUpdate();

  useEffect(() => {
    beimStartPruefen();
  }, []);

  if (!u.verfuegbar || (u.spaeter && u.phase === "ruhe")) return null;
  const laedt = u.phase === "laedt";

  return (
    <div className="update-card" role="status" title={`Installiert: Version ${version}`}>
      <div className="update-card-head">
        {Icon.update}
        <span>Neue Version {u.verfuegbar.version} verfügbar</span>
      </div>
      {laedt ? (
        <>
          <div className="update-card-sub">
            {u.prozent === 100 ? "Wird installiert – die App startet gleich neu." : `Wird heruntergeladen …${u.prozent != null ? ` ${u.prozent} %` : ""}`}
          </div>
          <UpdateBalken prozent={u.prozent} />
        </>
      ) : (
        <>
          {u.fehler && <div className="update-card-fehler">{u.fehler}</div>}
          <div className="update-card-actions">
            <button className="btn small mint" onClick={() => void installieren()}>
              Jetzt aktualisieren
            </button>
            <button className="link-btn" onClick={spaeter}>
              Später
            </button>
          </div>
        </>
      )}
    </div>
  );
}

export function UpdateBalken({ prozent }: { prozent: number | null }) {
  return (
    <div
      className={`update-bar ${prozent == null ? "unbestimmt" : ""}`}
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={prozent ?? undefined}
    >
      <span style={prozent != null ? { width: `${prozent}%` } : undefined} />
    </div>
  );
}
