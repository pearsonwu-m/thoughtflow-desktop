// Keyboard shortcuts: defaults, parsing, matching, and display.
//
// Accelerators use the same text format as Tauri's global shortcuts
// ("CmdOrCtrl+Shift+P", "Alt+Space") so one recorder serves both. Matching
// uses `KeyboardEvent.code`, which is stable even when ⌥ changes the
// character a key produces.

import type { ShortcutAction } from "../types";

export const DEFAULT_SHORTCUTS: Readonly<Record<ShortcutAction, string>> = {
  submit: "CmdOrCtrl+Enter",
  close: "Escape",
  clarify: "CmdOrCtrl+K",
  planMode: "CmdOrCtrl+P",
  taskMode: "CmdOrCtrl+T",
  reflectMode: "CmdOrCtrl+R",
  promptMode: "CmdOrCtrl+Shift+P",
  thinkMode: "CmdOrCtrl+I",
  history: "CmdOrCtrl+H",
  settings: "CmdOrCtrl+,",
  newThought: "CmdOrCtrl+N",
  save: "CmdOrCtrl+S",
  stop: "CmdOrCtrl+.",
};

export const SHORTCUT_LABELS: Readonly<Record<ShortcutAction, string>> = {
  submit: "Send to Claude",
  close: "Close the widget",
  clarify: "Ask for one clarifying question",
  planMode: "Plan mode",
  taskMode: "Task mode",
  reflectMode: "Reflect mode",
  promptMode: "Prompt mode",
  thinkMode: "Think mode",
  history: "Thought history",
  settings: "Settings",
  newThought: "New thought",
  save: "Save (thought, plan, or tasks)",
  stop: "Stop Claude's reply",
};

export const SHORTCUT_ORDER: readonly ShortcutAction[] = [
  "submit",
  "close",
  "clarify",
  "thinkMode",
  "planMode",
  "taskMode",
  "reflectMode",
  "promptMode",
  "history",
  "settings",
  "newThought",
  "save",
  "stop",
];

export interface Accelerator {
  meta: boolean;
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
  /** A `KeyboardEvent.code`, e.g. "KeyP", "Enter", "Comma". */
  code: string;
}

/** The subset of KeyboardEvent the matcher needs (easy to fake in tests). */
export interface KeyLike {
  metaKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  code: string;
}

export const isMac =
  typeof navigator === "undefined" ? true : /Mac|iPhone|iPad/i.test(navigator.platform || navigator.userAgent);

const KEY_TO_CODE: Record<string, string> = {
  enter: "Enter",
  return: "Enter",
  escape: "Escape",
  esc: "Escape",
  space: "Space",
  tab: "Tab",
  backspace: "Backspace",
  delete: "Delete",
  ",": "Comma",
  comma: "Comma",
  ".": "Period",
  period: "Period",
  "/": "Slash",
  slash: "Slash",
  ";": "Semicolon",
  semicolon: "Semicolon",
  "'": "Quote",
  quote: "Quote",
  "[": "BracketLeft",
  bracketleft: "BracketLeft",
  "]": "BracketRight",
  bracketright: "BracketRight",
  "-": "Minus",
  minus: "Minus",
  "=": "Equal",
  equal: "Equal",
  "`": "Backquote",
  backquote: "Backquote",
  up: "ArrowUp",
  arrowup: "ArrowUp",
  down: "ArrowDown",
  arrowdown: "ArrowDown",
  left: "ArrowLeft",
  arrowleft: "ArrowLeft",
  right: "ArrowRight",
  arrowright: "ArrowRight",
};

function keyToCode(key: string): string | null {
  const k = key.trim();
  if (!k) return null;
  const lower = k.toLowerCase();
  if (KEY_TO_CODE[lower]) return KEY_TO_CODE[lower];
  if (/^[a-z]$/i.test(k)) return `Key${k.toUpperCase()}`;
  if (/^key[a-z]$/i.test(k)) return `Key${k.slice(3).toUpperCase()}`;
  if (/^[0-9]$/.test(k)) return `Digit${k}`;
  if (/^digit[0-9]$/i.test(k)) return `Digit${k.slice(5)}`;
  if (/^f([1-9]|1[0-9]|2[0-4])$/i.test(k)) return k.toUpperCase();
  return null;
}

export function parseAccelerator(accelerator: string): Accelerator | null {
  // Split on "+" but keep a literal "+" key if one is ever used ("Cmd++").
  const parts = accelerator.split(/\+(?!$)/).map((p) => p.trim());
  const key = parts.pop();
  if (!key) return null;
  const acc: Accelerator = { meta: false, ctrl: false, alt: false, shift: false, code: "" };
  for (const part of parts) {
    switch (part.toLowerCase()) {
      case "cmd":
      case "command":
      case "super":
      case "meta":
        acc.meta = true;
        break;
      case "cmdorctrl":
      case "commandorcontrol":
      case "cmdorcontrol":
      case "commandorctrl":
        if (isMac) acc.meta = true;
        else acc.ctrl = true;
        break;
      case "ctrl":
      case "control":
        acc.ctrl = true;
        break;
      case "alt":
      case "option":
        acc.alt = true;
        break;
      case "shift":
        acc.shift = true;
        break;
      default:
        return null;
    }
  }
  const code = keyToCode(key);
  if (!code) return null;
  acc.code = code;
  return acc;
}

export function matchesAccelerator(event: KeyLike, accelerator: string): boolean {
  const acc = parseAccelerator(accelerator);
  if (!acc) return false;
  const code = event.code === "NumpadEnter" ? "Enter" : event.code;
  return (
    code === acc.code &&
    event.metaKey === acc.meta &&
    event.ctrlKey === acc.ctrl &&
    event.altKey === acc.alt &&
    event.shiftKey === acc.shift
  );
}

const CODE_LABELS: Record<string, string> = {
  Enter: "↵",
  Escape: "Esc",
  Space: "Space",
  Tab: "⇥",
  Backspace: "⌫",
  Delete: "⌦",
  Comma: ",",
  Period: ".",
  Slash: "/",
  Semicolon: ";",
  Quote: "'",
  BracketLeft: "[",
  BracketRight: "]",
  Minus: "-",
  Equal: "=",
  Backquote: "`",
  ArrowUp: "↑",
  ArrowDown: "↓",
  ArrowLeft: "←",
  ArrowRight: "→",
};

function codeLabel(code: string): string {
  if (CODE_LABELS[code]) return CODE_LABELS[code];
  if (code.startsWith("Key")) return code.slice(3);
  if (code.startsWith("Digit")) return code.slice(5);
  return code;
}

/** "CmdOrCtrl+Shift+P" → "⌘⇧P" (macOS symbol order: ⌃⌥⇧⌘). */
export function displayAccelerator(accelerator: string): string {
  const acc = parseAccelerator(accelerator);
  if (!acc) return accelerator;
  if (!isMac) {
    const mods = [acc.ctrl && "Ctrl", acc.alt && "Alt", acc.shift && "Shift", acc.meta && "Win"].filter(Boolean);
    return [...mods, codeLabel(acc.code)].join("+");
  }
  return `${acc.ctrl ? "⌃" : ""}${acc.alt ? "⌥" : ""}${acc.shift ? "⇧" : ""}${acc.meta ? "⌘" : ""}${codeLabel(acc.code)}`;
}

const MODIFIER_CODES = new Set([
  "MetaLeft",
  "MetaRight",
  "ControlLeft",
  "ControlRight",
  "AltLeft",
  "AltRight",
  "ShiftLeft",
  "ShiftRight",
  "CapsLock",
  "Fn",
]);

function codeToKeyName(code: string): string | null {
  if (code.startsWith("Key")) return code.slice(3);
  if (code.startsWith("Digit")) return code.slice(5);
  if (/^F\d{1,2}$/.test(code)) return code;
  if (code === "NumpadEnter") return "Enter";
  const named = ["Enter", "Escape", "Space", "Tab", "Backspace", "Delete", "Comma", "Period", "Slash", "Semicolon",
    "Quote", "BracketLeft", "BracketRight", "Minus", "Equal", "Backquote", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"];
  return named.includes(code) ? code : null;
}

/**
 * Converts a key press into an accelerator string for the shortcut recorder.
 * Returns null while only modifiers are held, or for keys that can't be bound.
 * `requireModifier` is used for the global shortcut, which must not swallow
 * plain typing system-wide.
 */
export function eventToAccelerator(event: KeyLike, requireModifier = false): string | null {
  if (MODIFIER_CODES.has(event.code)) return null;
  const key = codeToKeyName(event.code);
  if (!key) return null;
  const parts: string[] = [];
  if (event.metaKey) parts.push(isMac ? "CmdOrCtrl" : "Super");
  if (event.ctrlKey) parts.push(isMac ? "Ctrl" : "CmdOrCtrl");
  if (event.altKey) parts.push("Alt");
  if (event.shiftKey) parts.push("Shift");
  const hasModifier = event.metaKey || event.ctrlKey || event.altKey;
  if (requireModifier && !hasModifier && !/^F\d{1,2}$/.test(key)) return null;
  return [...parts, key].join("+");
}

export function resolveShortcuts(
  overrides: Partial<Record<ShortcutAction, string>> | undefined,
): Record<ShortcutAction, string> {
  const resolved = { ...DEFAULT_SHORTCUTS };
  for (const action of SHORTCUT_ORDER) {
    const value = overrides?.[action];
    if (value && parseAccelerator(value)) resolved[action] = value;
  }
  return resolved;
}

/** Actions that share a binding with an earlier action. */
export function findConflicts(bindings: Record<ShortcutAction, string>): ShortcutAction[] {
  const seen = new Map<string, ShortcutAction>();
  const conflicts: ShortcutAction[] = [];
  for (const action of SHORTCUT_ORDER) {
    const acc = parseAccelerator(bindings[action]);
    if (!acc) continue;
    const key = JSON.stringify(acc);
    if (seen.has(key)) conflicts.push(action);
    else seen.set(key, action);
  }
  return conflicts;
}

/** Finds which action (if any) a key event triggers. */
export function actionForEvent(
  event: KeyLike,
  bindings: Record<ShortcutAction, string>,
): ShortcutAction | null {
  for (const action of SHORTCUT_ORDER) {
    if (matchesAccelerator(event, bindings[action])) return action;
  }
  return null;
}
