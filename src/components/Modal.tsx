// Modaler Dialog (Hintergrund, Esc, Fokusfalle) und die Rückfrage bei ungespeicherten Änderungen.
import { useEffect, useId, useRef, type KeyboardEvent as ReactKeyboardEvent, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { createRoot } from "react-dom/client";
import { Icon } from "../icons";

// Stapel offener Dialoge: nur der oberste reagiert auf Esc.
const stack: number[] = [];
let nextId = 0;

/** true, solange irgendein Dialog offen ist (Strg+N / Strg+S werden dann ignoriert). */
export function isModalOpen(): boolean {
  return stack.length > 0;
}

const FOCUSABLE =
  'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [contenteditable="true"], [tabindex]:not([tabindex="-1"])';

export function Modal({
  onClose,
  children,
  className,
  labelledBy,
  onKeyDown,
}: {
  onClose: () => void;
  children: ReactNode;
  className?: string;
  labelledBy?: string;
  /** zusätzliche Tasten (z. B. Strg+S im Konfliktdialog) */
  onKeyDown?: (e: ReactKeyboardEvent<HTMLDivElement>) => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  useEffect(() => {
    nextId += 1;
    const id = nextId;
    stack.push(id);
    const previous = document.activeElement as HTMLElement | null;

    // Startfokus: ein Element mit autoFocus hat ihn schon, sonst das erste fokussierbare.
    const el = ref.current;
    if (el && !el.contains(document.activeElement)) {
      const first = el.querySelector<HTMLElement>("[data-autofocus]") ?? el.querySelector<HTMLElement>(FOCUSABLE);
      (first ?? el).focus();
    }

    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || stack[stack.length - 1] !== id || e.defaultPrevented) return;
      e.preventDefault();
      onCloseRef.current();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      const i = stack.indexOf(id);
      if (i >= 0) stack.splice(i, 1);
      if (previous && document.contains(previous)) previous.focus();
    };
  }, []);

  function trapFocus(e: ReactKeyboardEvent<HTMLDivElement>) {
    onKeyDown?.(e);
    if (e.key !== "Tab" || !ref.current) return;
    const items = Array.from(ref.current.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(
      (x) => x.offsetParent !== null || x === document.activeElement,
    );
    if (items.length === 0) {
      e.preventDefault();
      return;
    }
    const first = items[0];
    const last = items[items.length - 1];
    if (e.shiftKey && (document.activeElement === first || !ref.current.contains(document.activeElement))) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  }

  return createPortal(
    <div className="modal-backdrop">
      <div
        ref={ref}
        className={`modal glass ${className ?? ""}`}
        role="dialog"
        aria-modal="true"
        aria-labelledby={labelledBy}
        tabIndex={-1}
        onKeyDown={trapFocus}
      >
        {children}
      </div>
    </div>,
    document.body,
  );
}

export type UnsavedChoice = "save" | "discard" | "cancel";

/**
 * Rückfrage „Speichern / Verwerfen / Abbrechen“. Läuft in einer eigenen React-Wurzel,
 * damit sie auch aus dem Fenster-Schließen-Handler heraus funktioniert.
 * „Speichern“ fehlt, wenn `canSave === false`.
 */
export function askUnsaved(name: string, opts?: { canSave?: boolean; text?: string }): Promise<UnsavedChoice> {
  return new Promise((resolve) => {
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    let done = false;
    const finish = (c: UnsavedChoice) => {
      if (done) return;
      done = true;
      resolve(c);
      // nicht synchron im eigenen Ereignis-Handler aushängen
      setTimeout(() => {
        root.unmount();
        host.remove();
      }, 0);
    };
    root.render(<UnsavedDialog name={name} canSave={opts?.canSave !== false} text={opts?.text} onChoice={finish} />);
  });
}

function UnsavedDialog({
  name,
  canSave,
  text,
  onChoice,
}: {
  name: string;
  canSave: boolean;
  text?: string;
  onChoice: (c: UnsavedChoice) => void;
}) {
  const titleId = useId();
  return (
    <Modal onClose={() => onChoice("cancel")} className="modal-small" labelledBy={titleId}>
      <div className="modal-head">
        <h2 id={titleId} className="modal-title">
          {Icon.warning} Ungespeicherte Änderungen
        </h2>
      </div>
      <div className="modal-body">
        <p>
          {text ?? (
            <>
              „{name}“ hat ungespeicherte Änderungen.
              {canSave ? " Möchtest du sie speichern?" : " Speichern ist gerade nicht möglich – beim Verwerfen gehen sie verloren."}
            </>
          )}
        </p>
      </div>
      <div className="modal-foot">
        <button className="btn" onClick={() => onChoice("cancel")}>
          Abbrechen
        </button>
        <button className="btn danger" onClick={() => onChoice("discard")} data-autofocus={canSave ? undefined : true}>
          Verwerfen
        </button>
        {canSave && (
          <button className="btn primary" onClick={() => onChoice("save")} data-autofocus>
            {Icon.save} Speichern
          </button>
        )}
      </div>
    </Modal>
  );
}
