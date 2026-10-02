import { describe, expect, it } from "vitest";
import {
  actionForEvent,
  DEFAULT_SHORTCUTS,
  displayAccelerator,
  eventToAccelerator,
  findConflicts,
  matchesAccelerator,
  parseAccelerator,
  resolveShortcuts,
} from "./shortcuts";

const key = (code: string, mods: Partial<{ meta: boolean; ctrl: boolean; alt: boolean; shift: boolean }> = {}) => ({
  code,
  metaKey: !!mods.meta,
  ctrlKey: !!mods.ctrl,
  altKey: !!mods.alt,
  shiftKey: !!mods.shift,
});

describe("parseAccelerator", () => {
  it("parses modifiers and keys", () => {
    expect(parseAccelerator("CmdOrCtrl+Shift+P")).toEqual({ meta: true, ctrl: false, alt: false, shift: true, code: "KeyP" });
    expect(parseAccelerator("Alt+Space")?.code).toBe("Space");
    expect(parseAccelerator("CmdOrCtrl+,")?.code).toBe("Comma");
    expect(parseAccelerator("Escape")?.code).toBe("Escape");
  });

  it("rejects unknown pieces", () => {
    expect(parseAccelerator("Hyper+P")).toBeNull();
    expect(parseAccelerator("CmdOrCtrl+Banana")).toBeNull();
    expect(parseAccelerator("")).toBeNull();
  });
});

describe("matching", () => {
  it("distinguishes ⌘P from ⌘⇧P", () => {
    expect(matchesAccelerator(key("KeyP", { meta: true }), "CmdOrCtrl+P")).toBe(true);
    expect(matchesAccelerator(key("KeyP", { meta: true, shift: true }), "CmdOrCtrl+P")).toBe(false);
    expect(matchesAccelerator(key("KeyP", { meta: true, shift: true }), "CmdOrCtrl+Shift+P")).toBe(true);
  });

  it("treats the keypad Enter like Enter", () => {
    expect(matchesAccelerator(key("NumpadEnter", { meta: true }), "CmdOrCtrl+Enter")).toBe(true);
  });

  it("maps events to actions using the defaults", () => {
    const bindings = resolveShortcuts({});
    expect(actionForEvent(key("KeyT", { meta: true }), bindings)).toBe("taskMode");
    expect(actionForEvent(key("KeyP", { meta: true, shift: true }), bindings)).toBe("promptMode");
    expect(actionForEvent(key("Escape"), bindings)).toBe("close");
    expect(actionForEvent(key("KeyA"), bindings)).toBeNull();
  });
});

describe("display", () => {
  it("uses macOS symbols", () => {
    expect(displayAccelerator("CmdOrCtrl+Shift+P")).toBe("⇧⌘P");
    expect(displayAccelerator("Alt+Space")).toBe("⌥Space");
    expect(displayAccelerator("CmdOrCtrl+Enter")).toBe("⌘↵");
    expect(displayAccelerator("Escape")).toBe("Esc");
  });
});

describe("recording", () => {
  it("ignores bare modifiers and produces Tauri accelerators", () => {
    expect(eventToAccelerator(key("ShiftLeft", { shift: true }))).toBeNull();
    expect(eventToAccelerator(key("KeyJ", { meta: true, shift: true }))).toBe("CmdOrCtrl+Shift+J");
    expect(eventToAccelerator(key("Space", { alt: true }), true)).toBe("Alt+Space");
  });

  it("requires a modifier for global shortcuts", () => {
    expect(eventToAccelerator(key("KeyJ"), true)).toBeNull();
    expect(eventToAccelerator(key("F5"), true)).toBe("F5");
  });
});

describe("overrides", () => {
  it("keeps valid overrides and drops invalid ones", () => {
    const resolved = resolveShortcuts({ planMode: "CmdOrCtrl+Shift+L", taskMode: "nonsense" });
    expect(resolved.planMode).toBe("CmdOrCtrl+Shift+L");
    expect(resolved.taskMode).toBe(DEFAULT_SHORTCUTS.taskMode);
  });

  it("finds duplicate bindings", () => {
    expect(findConflicts(resolveShortcuts({}))).toEqual([]);
    expect(findConflicts(resolveShortcuts({ taskMode: "CmdOrCtrl+P" }))).toEqual(["taskMode"]);
  });
});
