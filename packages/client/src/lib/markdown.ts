// A small, safe Markdown reader for agent replies (the ACP session view, U07b). Agent text is untrusted: this builds a
// tree of plain data and nothing else, so the view renders it as text nodes and elements it chose itself. No HTML is
// ever passed through, a link is shown with the address it goes to, and only http(s) links are links at all.
// It reads the subset agents actually write: paragraphs, headings, lists, quotes, fenced code, inline code, emphasis,
// links and rules. Anything else stays as text, so nothing is lost.

export type Inline =
  | { t: 'text'; text: string; parts?: Part[] }
  | { t: 'code'; text: string; parts?: Part[] }
  | { t: 'b' | 'i'; inl: Inline[] }
  | { t: 'link'; text: string; href: string; parts?: Part[] };

/** A run of text with its search hits marked (see `decorate`). */
export interface Part { text: string; hit: boolean; current: boolean }

export type Block =
  | { t: 'p'; inl: Inline[] }
  | { t: 'h'; level: number; inl: Inline[] }
  | { t: 'ul' | 'ol'; items: Block[][]; start?: number }
  | { t: 'quote'; blocks: Block[] }
  | { t: 'code'; lang: string; text: string; parts?: Part[] }
  | { t: 'hr' };

const MAX_DEPTH = 4;
const MAX_BLOCKS = 600;

/** Only web links are links; the address is what the tap goes to, shown next to the words. */
export function safeHref(raw: string): string | null {
  try {
    const u = new URL(raw);
    return u.protocol === 'https:' || u.protocol === 'http:' ? u.href : null;
  } catch {
    return null;
  }
}

/** The host of a link, for showing where it goes. */
export function hostOf(href: string): string {
  try { return new URL(href).host; } catch { return ''; }
}

const INLINE = /(`+)([\s\S]*?[^`])\1(?!`)|\*\*([^*\n]+?)\*\*|__([^_\n]+?)__|(?<![*\w])\*([^*\s][^*\n]*?)\*(?![*\w])|(?<![_\w])_([^_\s][^_\n]*?)_(?![_\w])|\[([^\]\n]+)\]\(([^)\s]+)\)|(https?:\/\/[^\s<>)\]]+[^\s<>)\].,;:!?'"])/g;

export function parseInline(src: string, depth = 0): Inline[] {
  const out: Inline[] = [];
  let at = 0;
  const push = (text: string) => { if (text) out.push({ t: 'text', text }); };
  for (const m of src.matchAll(INLINE)) {
    const i = m.index ?? 0;
    push(src.slice(at, i));
    at = i + m[0].length;
    if (m[2] !== undefined) out.push({ t: 'code', text: m[2].replace(/^ (.*) $/s, '$1') });
    else if (m[3] !== undefined || m[4] !== undefined) out.push({ t: 'b', inl: depth < MAX_DEPTH ? parseInline(m[3] ?? m[4], depth + 1) : [{ t: 'text', text: m[3] ?? m[4] }] });
    else if (m[5] !== undefined || m[6] !== undefined) out.push({ t: 'i', inl: depth < MAX_DEPTH ? parseInline(m[5] ?? m[6], depth + 1) : [{ t: 'text', text: m[5] ?? m[6] }] });
    else if (m[7] !== undefined) {
      const href = safeHref(m[8]);
      if (href) out.push({ t: 'link', text: m[7], href });
      else push(m[0]);
    } else {
      const href = safeHref(m[9]);
      if (href) out.push({ t: 'link', text: m[9], href });
      else push(m[0]);
    }
  }
  push(src.slice(at));
  return out;
}

const FENCE = /^ {0,3}(`{3,}|~{3,})\s*([\w+#.-]*)\s*$/;
const HEADING = /^ {0,3}(#{1,6})\s+(.*?)\s*#*\s*$/;
const RULE = /^ {0,3}([-*_])(?:\s*\1){2,}\s*$/;
const BULLET = /^( *)([-*+])\s+(.*)$/;
const ORDERED = /^( *)(\d{1,9})[.)]\s+(.*)$/;
const QUOTE = /^ {0,3}>\s?(.*)$/;

export function parseBlocks(src: string, depth = 0): Block[] {
  const lines = src.replace(/\r\n?/g, '\n').split('\n');
  const out: Block[] = [];
  let i = 0;
  const startsBlock = (l: string) => FENCE.test(l) || HEADING.test(l) || RULE.test(l) || BULLET.test(l) || ORDERED.test(l) || QUOTE.test(l);
  while (i < lines.length && out.length < MAX_BLOCKS) {
    const line = lines[i];
    if (!line.trim()) { i++; continue; }
    const fence = FENCE.exec(line);
    if (fence) {
      const mark = fence[1];
      const body: string[] = [];
      i++;
      // An unclosed fence (the reply is still streaming) runs to the end.
      while (i < lines.length && !new RegExp(`^ {0,3}${mark[0]}{${mark.length},}\\s*$`).test(lines[i])) body.push(lines[i++]);
      i++;
      out.push({ t: 'code', lang: fence[2], text: body.join('\n') });
      continue;
    }
    const h = HEADING.exec(line);
    if (h) { out.push({ t: 'h', level: h[1].length, inl: parseInline(h[2]) }); i++; continue; }
    if (RULE.test(line)) { out.push({ t: 'hr' }); i++; continue; }
    if (QUOTE.test(line) && depth < MAX_DEPTH) {
      const body: string[] = [];
      while (i < lines.length && QUOTE.test(lines[i])) body.push(QUOTE.exec(lines[i++])![1]);
      out.push({ t: 'quote', blocks: parseBlocks(body.join('\n'), depth + 1) });
      continue;
    }
    const li = BULLET.exec(line) ?? ORDERED.exec(line);
    if (li && depth < MAX_DEPTH) {
      const ordered = !!ORDERED.exec(line) && !BULLET.exec(line);
      const indent = li[1].length;
      const items: Block[][] = [];
      const start = ordered ? Number(ORDERED.exec(line)![2]) : undefined;
      while (i < lines.length) {
        const m = (ordered ? ORDERED : BULLET).exec(lines[i]);
        if (!m || m[1].length !== indent) break;
        const body = [m[3]];
        i++;
        // Continuation: lines indented deeper than the marker belong to the item (nested lists, wrapped text, code).
        while (i < lines.length) {
          const l = lines[i];
          const lead = l.search(/\S/);
          if (l.trim() === '') {
            const next = lines[i + 1];
            if (next !== undefined && next.trim() && next.search(/\S/) > indent) { body.push(''); i++; continue; }
            break;
          }
          if (lead <= indent) break;
          body.push(l.slice(Math.min(indent + 2, lead)));
          i++;
        }
        items.push(parseBlocks(body.join('\n'), depth + 1));
      }
      out.push({ t: ordered ? 'ol' : 'ul', items, start });
      continue;
    }
    const para: string[] = [line];
    i++;
    while (i < lines.length && lines[i].trim() && !startsBlock(lines[i])) para.push(lines[i++]);
    out.push({ t: 'p', inl: parseInline(para.map((l) => l.trim()).join('\n')) });
  }
  return out;
}

// ---- Plain text and search over the same tree

function inlineText(inl: Inline[], out: string[]) {
  for (const n of inl) {
    if (n.t === 'b' || n.t === 'i') inlineText(n.inl, out);
    else out.push(n.text);
  }
}

/** The text runs of a tree in the order they are drawn: what the search looks through. */
export function runsOf(blocks: Block[], out: string[] = []): string[] {
  for (const b of blocks) {
    if (b.t === 'p' || b.t === 'h') inlineText(b.inl, out);
    else if (b.t === 'code') out.push(b.text);
    else if (b.t === 'quote') runsOf(b.blocks, out);
    else if (b.t === 'ul' || b.t === 'ol') for (const it of b.items) runsOf(it, out);
  }
  return out;
}

/** Hits of `query` (case-insensitive) in one run: [start, end) pairs. A hit never crosses a run. */
export function hitsIn(text: string, query: string): [number, number][] {
  if (!query) return [];
  const hay = text.toLowerCase();
  const needle = query.toLowerCase();
  const out: [number, number][] = [];
  for (let at = hay.indexOf(needle); at >= 0 && out.length < 500; at = hay.indexOf(needle, at + needle.length)) out.push([at, at + needle.length]);
  return out;
}

export function countHits(blocks: Block[], query: string): number {
  return runsOf(blocks).reduce((n, r) => n + hitsIn(r, query).length, 0);
}

/** A copy of the tree whose text runs carry `parts`, with the `current`-th hit (0-based, in drawing order) marked. */
export function decorate(blocks: Block[], query: string, current: number): Block[] {
  if (!query) return blocks;
  let seen = 0;
  const parts = (text: string): Part[] | undefined => {
    const hits = hitsIn(text, query);
    if (!hits.length) return undefined;
    const out: Part[] = [];
    let at = 0;
    for (const [a, b] of hits) {
      if (a > at) out.push({ text: text.slice(at, a), hit: false, current: false });
      out.push({ text: text.slice(a, b), hit: true, current: seen++ === current });
      at = b;
    }
    if (at < text.length) out.push({ text: text.slice(at), hit: false, current: false });
    return out;
  };
  const inl = (list: Inline[]): Inline[] => list.map((n) => (n.t === 'b' || n.t === 'i' ? { ...n, inl: inl(n.inl) } : { ...n, parts: parts(n.text) }));
  const walk = (list: Block[]): Block[] => list.map((b): Block => {
    if (b.t === 'p' || b.t === 'h') return { ...b, inl: inl(b.inl) };
    if (b.t === 'code') return { ...b, parts: parts(b.text) };
    if (b.t === 'quote') return { ...b, blocks: walk(b.blocks) };
    if (b.t === 'ul' || b.t === 'ol') return { ...b, items: b.items.map(walk) };
    return b;
  });
  return walk(blocks);
}

/** Splits any text into parts for a surface that is not Markdown (a diff line, a tool title). */
export function partsOf(text: string, query: string, firstIndex: number, current: number): Part[] | undefined {
  const hits = hitsIn(text, query);
  if (!hits.length) return undefined;
  const out: Part[] = [];
  let at = 0;
  let n = firstIndex;
  for (const [a, b] of hits) {
    if (a > at) out.push({ text: text.slice(at, a), hit: false, current: false });
    out.push({ text: text.slice(a, b), hit: true, current: n++ === current });
    at = b;
  }
  if (at < text.length) out.push({ text: text.slice(at), hit: false, current: false });
  return out;
}
