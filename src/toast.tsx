// Toast-Stapel unten rechts. Jeder Schreibvorgang endet mit einem Toast samt „In Obsidian öffnen“.
import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { api, errorText } from "./api";
import { Icon } from "./icons";

export type ToastKind = "live" | "saved" | "created" | "info" | "error";

export interface ToastInput {
  kind: ToastKind;
  text: string;
  /** Vault-relativer Pfad → Knopf „In Obsidian öffnen“ */
  path?: string;
  /** kleinere Zusatzzeilen (Warnungen, angelegte Ordner) */
  detail?: string[];
}

interface ToastEntry extends ToastInput {
  id: number;
}

type Notify = (t: ToastInput) => void;

const ToastContext = createContext<Notify>(() => {});

const MAX_SICHTBAR = 3;

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<ToastEntry[]>([]);
  const nextId = useRef(0);

  const remove = useCallback((id: number) => {
    setToasts((ts) => ts.filter((t) => t.id !== id));
  }, []);

  const notify = useCallback<Notify>((t) => {
    nextId.current += 1;
    const entry = { ...t, id: nextId.current };
    // neueste unten; höchstens drei sichtbar
    setToasts((ts) => [...ts, entry].slice(-MAX_SICHTBAR));
  }, []);

  return (
    <ToastContext.Provider value={notify}>
      {children}
      {toasts.length > 0 && (
        <div className="toast-stack">
          {toasts.map((t) => (
            <ToastItem key={t.id} toast={t} onDone={remove} notify={notify} />
          ))}
        </div>
      )}
    </ToastContext.Provider>
  );
}

export function useToast(): { notify: Notify } {
  const notify = useContext(ToastContext);
  return { notify };
}

const BADGE: Record<ToastKind, { cls: string; label: string }> = {
  live: { cls: "mint", label: "live" },
  saved: { cls: "mint", label: "gespeichert" },
  created: { cls: "mint", label: "angelegt" },
  info: { cls: "muted", label: "Info" },
  error: { cls: "red", label: "Fehler" },
};

function ToastItem({ toast, onDone, notify }: { toast: ToastEntry; onDone: (id: number) => void; notify: Notify }) {
  // Lebensdauer 3,2 s, mit Pfad 7 s; Hover hält den Zeitgeber an.
  const remaining = useRef(toast.path ? 7000 : 3200);
  const startedAt = useRef(0);
  const [paused, setPaused] = useState(false);

  useEffect(() => {
    if (paused) return;
    startedAt.current = Date.now();
    const t = setTimeout(() => onDone(toast.id), remaining.current);
    return () => {
      clearTimeout(t);
      remaining.current = Math.max(800, remaining.current - (Date.now() - startedAt.current));
    };
  }, [paused, onDone, toast.id]);

  const b = BADGE[toast.kind];
  const path = toast.path;

  return (
    <div
      className={`toast glass ${toast.kind}`}
      onMouseEnter={() => setPaused(true)}
      onMouseLeave={() => setPaused(false)}
      role={toast.kind === "error" ? "alert" : "status"}
    >
      <div className="toast-main">
        <span className={`badge ${b.cls}`}>
          {toast.kind === "live" && Icon.refresh}
          {toast.kind === "error" && Icon.warning}
          {b.label}
        </span>
        <span className="toast-text">{toast.text}</span>
        <button className="toast-close" onClick={() => onDone(toast.id)} title="Schließen" aria-label="Schließen">
          {Icon.close}
        </button>
      </div>
      {toast.detail && toast.detail.length > 0 && (
        <ul className="toast-detail">
          {toast.detail.map((d, i) => (
            <li key={i}>{d}</li>
          ))}
        </ul>
      )}
      {path && (
        <div className="toast-actions">
          <button
            className="btn small"
            onClick={() => api.openInObsidian(path).catch((e) => notify({ kind: "error", text: errorText(e) }))}
          >
            {Icon.obsidian} In Obsidian öffnen
          </button>
        </div>
      )}
    </div>
  );
}
