// A deliberately small Markdown parser for Claude's replies.
//
// Replies use light structure (short paragraphs, bold labels, lists, ###
// headings), so a full Markdown engine isn't needed. Output is a plain data
// tree rendered by components/Markdown.tsx; nothing is ever injected as HTML.

export type Inline =
  | { type: "text"; text: string }
  | { type: "strong"; children: Inline[] }
  | { type: "em"; children: Inline[] }
  | { type: "code"; text: string };

export type Block =
  | { type: "heading"; level: number; children: Inline[] }
  | { type: "paragraph"; children: Inline[] }
  | { type: "list"; ordered: boolean; start: number; items: Inline[][] }
  | { type: "code"; text: string }
  | { type: "quote"; children: Inline[] }
  | { type: "rule" };

export function parseInline(text: string): Inline[] {
  const out: Inline[] = [];
  let buffer = "";
  const flush = () => {
    if (buffer) out.push({ type: "text", text: buffer });
    buffer = "";
  };
  let i = 0;
  while (i < text.length) {
    const rest = text.slice(i);
    if (rest.startsWith("`")) {
      const end = text.indexOf("`", i + 1);
      if (end > i + 1) {
        flush();
        out.push({ type: "code", text: text.slice(i + 1, end) });
        i = end + 1;
        continue;
      }
    }
    if (rest.startsWith("**") || rest.startsWith("__")) {
      const marker = rest.slice(0, 2);
      const end = text.indexOf(marker, i + 2);
      if (end > i + 2) {
        flush();
        out.push({ type: "strong", children: parseInline(text.slice(i + 2, end)) });
        i = end + 2;
        continue;
      }
    }
    const ch = text[i];
    if ((ch === "*" || ch === "_") && text[i + 1] && text[i + 1] !== " ") {
      // Avoid treating snake_case or a lone asterisk as emphasis.
      const prev = i > 0 ? text[i - 1] : " ";
      const end = text.indexOf(ch, i + 1);
      if (end > i + 1 && /[\s(["'“]/.test(prev ?? " ") && text[end - 1] !== " ") {
        flush();
        out.push({ type: "em", children: parseInline(text.slice(i + 1, end)) });
        i = end + 1;
        continue;
      }
    }
    buffer += ch;
    i += 1;
  }
  flush();
  return out;
}

const LIST_ITEM = /^\s*(?:([-*•])|(\d+)[.)])\s+(.*)$/;

export function parseMarkdown(source: string): Block[] {
  const lines = source.replace(/\r\n?/g, "\n").split("\n");
  const blocks: Block[] = [];
  let paragraph: string[] = [];

  const flushParagraph = () => {
    if (paragraph.length) {
      blocks.push({ type: "paragraph", children: parseInline(paragraph.join(" ")) });
      paragraph = [];
    }
  };

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i] ?? "";
    const trimmed = line.trim();

    if (trimmed.startsWith("```")) {
      flushParagraph();
      const code: string[] = [];
      i++;
      while (i < lines.length && !(lines[i] ?? "").trim().startsWith("```")) {
        code.push(lines[i] ?? "");
        i++;
      }
      blocks.push({ type: "code", text: code.join("\n") });
      continue;
    }
    if (!trimmed) {
      flushParagraph();
      continue;
    }
    const heading = /^(#{1,6})\s+(.*)$/.exec(trimmed);
    if (heading) {
      flushParagraph();
      blocks.push({ type: "heading", level: heading[1]?.length ?? 3, children: parseInline(heading[2] ?? "") });
      continue;
    }
    if (/^(-{3,}|\*{3,}|_{3,})$/.test(trimmed)) {
      flushParagraph();
      blocks.push({ type: "rule" });
      continue;
    }
    if (trimmed.startsWith(">")) {
      flushParagraph();
      blocks.push({ type: "quote", children: parseInline(trimmed.replace(/^>\s?/, "")) });
      continue;
    }
    const item = LIST_ITEM.exec(line);
    if (item) {
      flushParagraph();
      const ordered = Boolean(item[2]);
      const list: Block & { type: "list" } = {
        type: "list",
        ordered,
        start: ordered ? Number(item[2]) : 1,
        items: [parseInline(item[3] ?? "")],
      };
      // Gather following items of the same kind, plus indented continuation lines.
      while (i + 1 < lines.length) {
        const next = lines[i + 1] ?? "";
        const nextItem = LIST_ITEM.exec(next);
        if (nextItem && Boolean(nextItem[2]) === ordered) {
          list.items.push(parseInline(nextItem[3] ?? ""));
          i++;
        } else if (next.trim() && /^\s{2,}/.test(next) && !nextItem) {
          const last = list.items[list.items.length - 1];
          last?.push({ type: "text", text: " " }, ...parseInline(next.trim()));
          i++;
        } else if (nextItem) {
          // Nested list of the other kind: flatten into this list.
          list.items.push(parseInline(nextItem[3] ?? ""));
          i++;
        } else {
          break;
        }
      }
      blocks.push(list);
      continue;
    }
    paragraph.push(trimmed);
  }
  flushParagraph();
  return blocks;
}

/** Splits a reply into text and generated-prompt segments. */
export type Segment = { type: "text"; text: string } | { type: "prompt"; text: string; complete: boolean };

export function splitPromptBlocks(reply: string): Segment[] {
  const segments: Segment[] = [];
  let rest = reply;
  while (rest.length) {
    const start = rest.indexOf("<prompt>");
    if (start === -1) {
      segments.push({ type: "text", text: rest });
      break;
    }
    if (start > 0) segments.push({ type: "text", text: rest.slice(0, start) });
    const afterOpen = rest.slice(start + "<prompt>".length);
    const end = afterOpen.indexOf("</prompt>");
    if (end === -1) {
      // Still streaming: show the partial prompt.
      segments.push({ type: "prompt", text: afterOpen.trim(), complete: false });
      break;
    }
    segments.push({ type: "prompt", text: afterOpen.slice(0, end).trim(), complete: true });
    rest = afterOpen.slice(end + "</prompt>".length);
  }
  return segments.filter((s) => s.type === "prompt" || s.text.trim().length > 0);
}

/** Plain text of inline nodes (for accessibility labels and tests). */
export function inlineText(nodes: Inline[]): string {
  return nodes.map((n) => (n.type === "text" || n.type === "code" ? n.text : inlineText(n.children))).join("");
}
