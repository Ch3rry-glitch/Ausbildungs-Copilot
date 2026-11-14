// Sichere Vorschau des aktuellen Puffers. Links haben kein href; ein Klick öffnet nur
// http/https/mailto über das Opener-Plugin – die Webview navigiert nie.
import { useMemo, type KeyboardEvent, type MouseEvent } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { errorText } from "../api";
import { renderMarkdown, splitFrontmatter } from "../markdown";
import { useToast } from "../toast";

const ERLAUBT = new Set(["http:", "https:", "mailto:"]);

export function MarkdownPreview({ text }: { text: string }) {
  const { notify } = useToast();
  const { frontmatter, html } = useMemo(() => {
    const { frontmatter, body } = splitFrontmatter(text);
    return { frontmatter, html: renderMarkdown(body) };
  }, [text]);

  function follow(target: EventTarget | null) {
    const el = target instanceof Element ? target.closest<HTMLElement>("[data-href]") : null;
    if (!el) return;
    const href = el.dataset.href ?? "";
    let url: URL;
    try {
      url = new URL(href);
    } catch {
      return; // relative Links, Anker usw.: nichts tun
    }
    if (!ERLAUBT.has(url.protocol)) return;
    openUrl(url.href).catch((e) => notify({ kind: "error", text: errorText(e) }));
  }

  function onClick(e: MouseEvent<HTMLDivElement>) {
    if (!(e.target instanceof Element) || !e.target.closest("[data-href]")) return;
    e.preventDefault();
    follow(e.target);
  }

  function onAuxClick(e: MouseEvent<HTMLDivElement>) {
    if (!(e.target instanceof Element) || !e.target.closest("[data-href]")) return;
    e.preventDefault();
    if (e.button === 1) follow(e.target);
  }

  function onKeyDown(e: KeyboardEvent<HTMLDivElement>) {
    if (e.key !== "Enter" || !(e.target instanceof Element) || !e.target.closest("[data-href]")) return;
    e.preventDefault();
    follow(e.target);
  }

  return (
    <div className="md-preview" onClick={onClick} onAuxClick={onAuxClick} onKeyDown={onKeyDown}>
      {frontmatter !== null && <pre className="fm">{frontmatter}</pre>}
      <div dangerouslySetInnerHTML={{ __html: html }} />
    </div>
  );
}
