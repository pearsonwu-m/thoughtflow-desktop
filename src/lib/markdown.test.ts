import { describe, expect, it } from "vitest";
import { inlineText, parseInline, parseMarkdown, splitPromptBlocks } from "./markdown";

describe("parseInline", () => {
  it("handles bold, italics, and code", () => {
    expect(parseInline("**What I'm hearing:** you want *time* for `code`")).toEqual([
      { type: "strong", children: [{ type: "text", text: "What I'm hearing:" }] },
      { type: "text", text: " you want " },
      { type: "em", children: [{ type: "text", text: "time" }] },
      { type: "text", text: " for " },
      { type: "code", text: "code" },
    ]);
  });

  it("leaves snake_case and stray asterisks alone", () => {
    expect(inlineText(parseInline("use snake_case_names and 2 * 3"))).toBe("use snake_case_names and 2 * 3");
    expect(parseInline("use snake_case_names").length).toBe(1);
  });
});

describe("parseMarkdown", () => {
  it("parses the Plan mode structure", () => {
    const blocks = parseMarkdown(
      "### Objective\nShip the site.\n\n### Steps\n1. Pick a template\n2. Write the About page\n\n- risk one\n- risk two",
    );
    expect(blocks.map((b) => b.type)).toEqual(["heading", "paragraph", "heading", "list", "list"]);
    const steps = blocks[3];
    expect(steps?.type === "list" && steps.ordered && steps.items.length).toBe(2);
  });

  it("keeps fenced code verbatim", () => {
    const blocks = parseMarkdown("```\n**not bold**\n```");
    expect(blocks).toEqual([{ type: "code", text: "**not bold**" }]);
  });

  it("joins soft-wrapped paragraph lines", () => {
    const blocks = parseMarkdown("one\ntwo\n\nthree");
    expect(blocks.length).toBe(2);
  });
});

describe("splitPromptBlocks", () => {
  it("separates generated prompts from commentary", () => {
    const segments = splitPromptBlocks("Here it is.\n<prompt>\nYou are a historian.\n</prompt>\nAssumed: essay.");
    expect(segments).toEqual([
      { type: "text", text: "Here it is.\n" },
      { type: "prompt", text: "You are a historian.", complete: true },
      { type: "text", text: "\nAssumed: essay." },
    ]);
  });

  it("shows partial prompts while streaming", () => {
    expect(splitPromptBlocks("<prompt>You are")).toEqual([{ type: "prompt", text: "You are", complete: false }]);
  });
});
