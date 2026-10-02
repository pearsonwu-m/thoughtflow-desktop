import { describe, expect, it } from "vitest";
import type { ThoughtDetail } from "../types";
import { hasConversation, initialState, reducer, type PendingTurn, type WidgetState } from "./widget";

const detail = (id = "t1"): ThoughtDetail => ({
  thought: {
    id,
    title: "Club",
    rawInput: "start a club",
    summary: "",
    tags: [],
    mode: "plan",
    includeInMemory: true,
    createdAt: 1,
    updatedAt: 1,
  },
  messages: [],
  plans: [],
  tasks: [],
  prompts: [],
});

const pending = (overrides: Partial<PendingTurn> = {}): PendingTurn => ({
  requestId: "r1",
  kind: "message",
  mode: "think",
  userText: "I need to finish physics",
  reply: "",
  phase: "sending",
  model: null,
  via: null,
  retryReason: null,
  regenerate: false,
  ...overrides,
});

describe("widget reducer", () => {
  it("streams a reply and clears the draft", () => {
    let s: WidgetState = { ...initialState, draft: "I need to finish physics" };
    s = reducer(s, { type: "sendStarted", pending: pending() });
    expect(s.draft).toBe("");
    s = reducer(s, { type: "stream", requestId: "r1", event: { type: "started", model: "claude-opus-5-5" } });
    expect(s.pending?.phase).toBe("thinking");
    s = reducer(s, { type: "stream", requestId: "r1", event: { type: "delta", text: "What " } });
    s = reducer(s, { type: "stream", requestId: "r1", event: { type: "delta", text: "I'm hearing" } });
    expect(s.pending?.reply).toBe("What I'm hearing");
    expect(s.pending?.phase).toBe("writing");
    expect(hasConversation(s)).toBe(true);
    s = reducer(s, { type: "sendSucceeded", requestId: "r1", detail: detail() });
    expect(s.pending).toBeNull();
    expect(s.detail?.thought.id).toBe("t1");
  });

  it("ignores events from a stale request", () => {
    const s = reducer({ ...initialState, pending: pending() }, {
      type: "stream",
      requestId: "other",
      event: { type: "delta", text: "x" },
    });
    expect(s.pending?.reply).toBe("");
  });

  it("restores the user's words when sending fails", () => {
    let s = reducer({ ...initialState, draft: "my thought" }, { type: "sendStarted", pending: pending({ userText: "my thought" }) });
    s = reducer(s, {
      type: "sendFailed",
      requestId: "r1",
      error: { kind: "network", message: "Claude couldn't be reached.", retryable: true },
    });
    expect(s.draft).toBe("my thought");
    expect(s.error?.kind).toBe("network");
    expect(s.pending).toBeNull();
  });

  it("opens thoughts in their last mode and won't switch mid-reply", () => {
    let s = reducer(initialState, { type: "openThought", detail: detail() });
    expect(s.mode).toBe("plan");
    s = reducer({ ...s, pending: pending() }, { type: "openThought", detail: detail("t2") });
    expect(s.detail?.thought.id).toBe("t1");
  });

  it("forgets a deleted thought that is open", () => {
    let s = reducer(initialState, { type: "openThought", detail: detail() });
    s = reducer(s, { type: "thoughtDeleted", id: "t1" });
    expect(s.detail).toBeNull();
    expect(s.mode).toBe("think");
  });
});
