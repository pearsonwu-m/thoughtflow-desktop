// Application state for the floating widget, as a pure reducer.

import type { AppError, Mode, SendKind, StreamEvent, ThoughtDetail } from "../types";

export type View = "thought" | "archive";
export type ArchiveTab = "thoughts" | "plans" | "tasks";

/** A turn that has been sent but not yet saved. */
export interface PendingTurn {
  requestId: string;
  kind: SendKind;
  mode: Mode;
  /** What the user sees as their side of the turn. */
  userText: string;
  /** The reply streamed so far. */
  reply: string;
  phase: "sending" | "thinking" | "writing" | "retrying";
  model: string | null;
  retryReason: string | null;
  /** Regenerating replaces the latest reply instead of adding a turn. */
  regenerate: boolean;
}

export type Busy = null | "plan" | "tasks" | "capture";

export interface WidgetState {
  view: View;
  archiveTab: ArchiveTab;
  mode: Mode;
  detail: ThoughtDetail | null;
  draft: string;
  pending: PendingTurn | null;
  error: AppError | null;
  notice: string | null;
  busy: Busy;
}

export const initialState: WidgetState = {
  view: "thought",
  archiveTab: "thoughts",
  mode: "think",
  detail: null,
  draft: "",
  pending: null,
  error: null,
  notice: null,
  busy: null,
};

export type Action =
  | { type: "newThought" }
  | { type: "openThought"; detail: ThoughtDetail }
  | { type: "detailUpdated"; detail: ThoughtDetail }
  | { type: "thoughtDeleted"; id: string }
  | { type: "setDraft"; text: string }
  | { type: "setMode"; mode: Mode }
  | { type: "setView"; view: View; tab?: ArchiveTab }
  | { type: "sendStarted"; pending: PendingTurn }
  | { type: "stream"; requestId: string; event: StreamEvent }
  | { type: "sendSucceeded"; requestId: string; detail: ThoughtDetail }
  | { type: "sendFailed"; requestId: string; error: AppError | null }
  | { type: "error"; error: AppError | null }
  | { type: "notice"; text: string | null }
  | { type: "busy"; busy: Busy }
  | { type: "dataCleared" };

export function reducer(state: WidgetState, action: Action): WidgetState {
  switch (action.type) {
    case "newThought":
      if (state.pending) return { ...state, view: "thought" };
      return { ...initialState, archiveTab: state.archiveTab };
    case "openThought":
      if (state.pending) return state;
      return {
        ...state,
        view: "thought",
        detail: action.detail,
        mode: action.detail.thought.mode,
        draft: "",
        error: null,
        notice: null,
      };
    case "detailUpdated":
      if (state.detail?.thought.id !== action.detail.thought.id) return state;
      return { ...state, detail: action.detail };
    case "thoughtDeleted":
      if (state.detail?.thought.id !== action.id) return state;
      return { ...state, detail: null, draft: "", mode: "think", pending: null };
    case "setDraft":
      return { ...state, draft: action.text };
    case "setMode":
      return { ...state, mode: action.mode };
    case "setView":
      return { ...state, view: action.view, archiveTab: action.tab ?? state.archiveTab, notice: null };
    case "sendStarted":
      return {
        ...state,
        pending: action.pending,
        mode: action.pending.mode,
        draft: action.pending.kind === "message" ? "" : state.draft,
        error: null,
        notice: null,
      };
    case "stream": {
      const p = state.pending;
      if (!p || p.requestId !== action.requestId) return state;
      const e = action.event;
      switch (e.type) {
        case "sending":
          return { ...state, pending: { ...p, phase: "sending", model: e.model } };
        case "started":
          return { ...state, pending: { ...p, phase: p.reply ? "writing" : "thinking", model: e.model, retryReason: null } };
        case "thinking":
          return { ...state, pending: { ...p, phase: p.reply ? "writing" : "thinking" } };
        case "retrying":
          return { ...state, pending: { ...p, phase: "retrying", retryReason: e.reason } };
        case "delta":
          return { ...state, pending: { ...p, phase: "writing", reply: p.reply + e.text } };
        default:
          return state;
      }
    }
    case "sendSucceeded":
      if (state.pending?.requestId !== action.requestId) return state;
      return { ...state, pending: null, detail: action.detail, mode: state.pending.mode };
    case "sendFailed": {
      const p = state.pending;
      if (!p || p.requestId !== action.requestId) return state;
      // Give the user their words back so nothing typed is lost.
      const restore = p.kind === "message" && !p.regenerate && !state.draft.trim();
      return { ...state, pending: null, error: action.error, draft: restore ? p.userText : state.draft };
    }
    case "error":
      return { ...state, error: action.error };
    case "notice":
      return { ...state, notice: action.text };
    case "busy":
      return { ...state, busy: action.busy };
    case "dataCleared":
      return { ...initialState };
    default:
      return state;
  }
}

/** Whether the widget is showing a conversation (vs. the empty capture state). */
export function hasConversation(state: WidgetState): boolean {
  return Boolean(state.pending || (state.detail && state.detail.messages.length > 0));
}
