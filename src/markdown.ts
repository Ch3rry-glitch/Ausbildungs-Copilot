// Vorschau-Renderer: kein rohes HTML in die IPC-Webview.
// Erste Verteidigungslinie: jedes von den Plugins erzeugte Stück Text geht durch escapeHtml.
// Zweite: DOMPurify über das fertige HTML.
import MarkdownIt from "markdown-it";
import DOMPurify from "dompurify";

type Md = InstanceType<typeof MarkdownIt>;
type StateInline = Parameters<Parameters<Md["inline"]["ruler"]["push"]>[1]>[0];
type StateCore = Parameters<Parameters<Md["core"]["ruler"]["push"]>[1]>[0];

const md: Md = new MarkdownIt({ html: false, linkify: false, typographer: false, breaks: false });
const esc = (s: string) => md.utils.escapeHtml(s);

// ---------- Wikilinks und Einbettungen: [[Ziel|Alias]], [[Ziel#Abschnitt|Alias]], ![[Datei]] ----------
function wikilink(state: StateInline, silent: boolean): boolean {
  const src = state.src;
  let pos = state.pos;
  const embed = src.charCodeAt(pos) === 0x21; /* ! */
  if (embed) pos += 1;
  if (src.charCodeAt(pos) !== 0x5b || src.charCodeAt(pos + 1) !== 0x5b) return false; /* [[ */
  const start = pos + 2;
  const end = src.indexOf("]]", start);
  if (end < 0) return false;
  // In Tabellen steht der Alias-Strich als "\|".
  const inner = src.slice(start, end).replace(/\\\|/g, "|");
  // leeres „[[]]“ (Vorlagen-Platzhalter) bleibt wörtlicher Text
  if (!inner.trim() || inner.includes("\n") || inner.includes("[[")) return false;
  if (!silent) {
    const bar = inner.indexOf("|");
    const target = (bar >= 0 ? inner.slice(0, bar) : inner).trim();
    const alias = bar >= 0 ? inner.slice(bar + 1).trim() : "";
    const tok = state.push(embed ? "cockpit_embed" : "cockpit_wikilink", "", 0);
    tok.meta = { target, alias };
  }
  state.pos = end + 2;
  return true;
}

// ---------- #tags (gleiche Regel wie tags.rs) ----------
const TAG_BODY = /[\p{L}\p{N}_/-]+/uy;
function tag(state: StateInline, silent: boolean): boolean {
  const src = state.src;
  const pos = state.pos;
  if (src.charCodeAt(pos) !== 0x23) return false; /* # */
  if (pos > 0 && !/[\s(\[,;]/.test(src[pos - 1])) return false;
  TAG_BODY.lastIndex = pos + 1;
  const m = TAG_BODY.exec(src);
  if (!m) return false;
  const name = m[0].replace(/[/-]+$/, "");
  if (!name || /^\d+$/.test(name)) return false;
  if (!silent) {
    const tok = state.push("cockpit_tag", "", 0);
    tok.meta = { tag: name };
  }
  state.pos = pos + 1 + name.length;
  return true;
}

// ---------- Aufgabenlisten: „[ ] “ / „[x] “ am Anfang eines Listenpunkts ----------
function aufgaben(state: StateCore): void {
  const tokens = state.tokens;
  for (let i = 2; i < tokens.length; i++) {
    const tk = tokens[i];
    if (tk.type !== "inline" || tokens[i - 1].type !== "paragraph_open" || tokens[i - 2].type !== "list_item_open") continue;
    const m = /^\[([ xX])\](?:\s|$)/.exec(tk.content);
    if (!m) continue;
    tk.content = (m[1] === " " ? "☐ " : "☑ ") + tk.content.slice(m[0].length);
  }
}

md.inline.ruler.before("link", "cockpit_wikilink", wikilink);
md.inline.ruler.before("emphasis", "cockpit_tag", tag);
md.core.ruler.before("inline", "cockpit_aufgaben", aufgaben);

md.renderer.rules.cockpit_wikilink = (tokens, idx) => {
  const { target, alias } = tokens[idx].meta as { target: string; alias: string };
  return `<span class="wikilink" title="${esc(target)}">${esc(alias || target)}</span>`;
};
md.renderer.rules.cockpit_embed = (tokens, idx) => {
  const { target } = tokens[idx].meta as { target: string };
  return `<span class="embed">📎 ${esc(target)}</span>`;
};
md.renderer.rules.cockpit_tag = (tokens, idx) => {
  const { tag: name } = tokens[idx].meta as { tag: string };
  return `<span class="tag">#${esc(name)}</span>`;
};

// Links ohne href: kein Mittelklick/Umschalt-Klick/Ziehen kann ein WebView2-Fenster öffnen.
md.renderer.rules.link_open = (tokens, idx) => {
  const href = String(tokens[idx].attrGet("href") ?? "");
  return `<span class="md-link" data-href="${esc(href)}" role="link" tabindex="0">`;
};
md.renderer.rules.link_close = () => "</span>";

// Bilder werden nie geladen (img-src bleibt 'self' data:).
md.renderer.rules.image = (tokens, idx) => {
  const alt = tokens[idx].content || String(tokens[idx].attrGet("src") ?? "");
  return `<span class="embed">🖼 ${esc(alt)}</span>`;
};

/** Frontmatter am Dateianfang abtrennen (`---` … `---` bzw. `...`). */
export function splitFrontmatter(text: string): { frontmatter: string | null; body: string } {
  const m = /^---[ \t]*\n(?:([\s\S]*?)\n)?(?:---|\.\.\.)[ \t]*(?:\n|$)/.exec(text);
  if (!m) return { frontmatter: null, body: text };
  return { frontmatter: m[1] ?? "", body: text.slice(m[0].length) };
}

const PURIFY = {
  USE_PROFILES: { html: true },
  FORBID_TAGS: ["a", "style", "form", "input", "button", "iframe", "img"],
  FORBID_ATTR: ["style", "href", "src"],
};

/** Markdown-Text (ohne Frontmatter) → bereinigtes HTML */
export function renderMarkdown(body: string): string {
  return DOMPurify.sanitize(md.render(body), PURIFY) as string;
}
