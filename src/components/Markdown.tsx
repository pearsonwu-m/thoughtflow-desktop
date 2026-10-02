import { memo, type ReactNode } from "react";
import { parseMarkdown, type Block, type Inline } from "../lib/markdown";

function renderInline(nodes: Inline[], keyPrefix: string): ReactNode[] {
  return nodes.map((node, i) => {
    const key = `${keyPrefix}.${i}`;
    switch (node.type) {
      case "text":
        return node.text;
      case "code":
        return <code key={key}>{node.text}</code>;
      case "strong":
        return <strong key={key}>{renderInline(node.children, key)}</strong>;
      case "em":
        return <em key={key}>{renderInline(node.children, key)}</em>;
      default:
        return null;
    }
  });
}

function renderBlock(block: Block, i: number): ReactNode {
  const key = String(i);
  switch (block.type) {
    case "heading": {
      const children = renderInline(block.children, key);
      if (block.level <= 2) return <h2 key={key}>{children}</h2>;
      if (block.level === 3) return <h3 key={key}>{children}</h3>;
      return <h4 key={key}>{children}</h4>;
    }
    case "paragraph":
      return <p key={key}>{renderInline(block.children, key)}</p>;
    case "list": {
      const items = block.items.map((item, j) => <li key={j}>{renderInline(item, `${key}.${j}`)}</li>);
      return block.ordered ? (
        <ol key={key} start={block.start}>
          {items}
        </ol>
      ) : (
        <ul key={key}>{items}</ul>
      );
    }
    case "code":
      return <pre key={key}>{block.text}</pre>;
    case "quote":
      return <blockquote key={key}>{renderInline(block.children, key)}</blockquote>;
    case "rule":
      return <hr key={key} />;
    default:
      return null;
  }
}

/** Renders Claude's Markdown as React elements (never as raw HTML). */
export const Markdown = memo(function Markdown({ text, className }: { text: string; className?: string }) {
  const blocks = parseMarkdown(text);
  return <div className={className ? `md ${className}` : "md"}>{blocks.map(renderBlock)}</div>;
});
