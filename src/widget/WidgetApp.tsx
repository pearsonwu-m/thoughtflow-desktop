import { useCallback, useEffect, useLayoutEffect, useReducer, useRef, useState } from "react";
import { useBindings, useSettings, useTheme } from "../hooks/useSettings";
import { api, confirmAction, copyText, newRequestId, onBackendEvent, toAppError } from "../lib/api";
import { isoDate, localDateLabel } from "../lib/format";
import { actionForEvent } from "../lib/shortcuts";
import { hasConversation, initialState, reducer, type ArchiveTab, type WidgetState } from "../state/widget";
import type { ApiKeyStatus, AppError, AppInfo, Mode, Plan, Prompt, RelatedNote, SendKind, ShortcutAction, ShortcutStatus, Task } from "../types";
import { Archive } from "./Archive";
import { Composer } from "./Composer";
import { ConversationView } from "./ConversationView";
import { Header } from "./Header";

const ARCHIVE_HEIGHT = 540;

const MODE_FOR_ACTION: Partial<Record<ShortcutAction, Mode>> = {
  thinkMode: "think",
  planMode: "plan",
  taskMode: "task",
  reflectMode: "reflect",
  promptMode: "prompt",
};

function conversationMaxHeight(): number {
  // Leave room for the header and composer on small screens.
  const available = window.screen?.availHeight ?? 900;
  return Math.max(200, Math.min(460, Math.round(available * 0.62) - 220));
}

export function WidgetApp() {
  const { settings } = useSettings();
  useTheme(settings);
  const bindings = useBindings(settings);
  const [state, dispatch] = useReducer(reducer, initialState);
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [motion, setMotion] = useState<"entering" | "leaving" | "">("entering");
  const [related, setRelated] = useState<RelatedNote[]>([]);
  const [excludedRelated, setExcludedRelated] = useState<Set<string>>(new Set());

  const cardRef = useRef<HTMLDivElement>(null);
  const composerRef = useRef<HTMLTextAreaElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  // Handlers registered once read the latest values through refs.
  const stateRef = useRef<WidgetState>(state);
  const bindingsRef = useRef(bindings);
  const relatedRef = useRef({ related, excludedRelated });
  useEffect(() => {
    stateRef.current = state;
    bindingsRef.current = bindings;
    relatedRef.current = { related, excludedRelated };
  });

  const useMemory = settings?.claude.useMemory ?? true;

  // --- Backend info and events ---------------------------------------------

  const refreshInfo = useCallback(() => {
    api.getAppInfo().then(setInfo).catch(() => {});
  }, []);

  const focusPrimary = useCallback(() => {
    requestAnimationFrame(() => {
      if (stateRef.current.view === "archive") searchRef.current?.focus();
      else composerRef.current?.focus();
    });
  }, []);

  useEffect(() => {
    refreshInfo();
    const offs = [
      onBackendEvent<{ view: string | null }>("tf://shown", ({ view }) => {
        setMotion("entering");
        if (view === "new") dispatch({ type: "newThought" });
        else if (view === "history") dispatch({ type: "setView", view: "archive", tab: "thoughts" });
        else if (view === "compose") dispatch({ type: "setView", view: "thought" });
        refreshInfo();
        focusPrimary();
      }),
      onBackendEvent("tf://hiding", () => setMotion("leaving")),
      onBackendEvent("tf://data-cleared", () => dispatch({ type: "dataCleared" })),
      onBackendEvent<ApiKeyStatus>("tf://api-key-changed", (apiKey) => {
        setInfo((i) => (i ? { ...i, apiKey } : i));
        dispatch({ type: "error", error: null });
      }),
      onBackendEvent<ShortcutStatus>("tf://shortcut-status", (shortcut) => setInfo((i) => (i ? { ...i, shortcut } : i))),
    ];
    focusPrimary();
    return () => offs.forEach((off) => off());
  }, [refreshInfo, focusPrimary]);

  // --- Window sizing: the window always hugs the card ------------------------

  useLayoutEffect(() => {
    const card = cardRef.current;
    if (!card) return;
    let last = 0;
    let frame = 0;
    const observer = new ResizeObserver(() => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        const height = card.offsetHeight;
        if (height !== last) {
          last = height;
          api.resizeWidget(height).catch(() => {});
        }
      });
    });
    observer.observe(card);
    return () => {
      observer.disconnect();
      cancelAnimationFrame(frame);
    };
  }, []);

  // --- Related memory for the first message of a thought ---------------------

  const isNewThought = !state.detail || state.detail.messages.length === 0;
  const wantsRelated = useMemory && state.view === "thought" && (isNewThought || state.mode === "reflect");
  useEffect(() => {
    if (!wantsRelated || state.draft.trim().length < 12) {
      setRelated([]);
      return;
    }
    const text = state.draft;
    const exclude = state.detail?.thought.id ?? null;
    const t = window.setTimeout(() => {
      api
        .findRelated(text, exclude)
        .then(setRelated)
        .catch(() => setRelated([]));
    }, 350);
    return () => window.clearTimeout(t);
  }, [state.draft, wantsRelated, state.detail?.thought.id]);

  useEffect(() => setExcludedRelated(new Set()), [state.detail?.thought.id]);

  // --- Actions ----------------------------------------------------------------

  const fail = useCallback((e: unknown) => dispatch({ type: "error", error: toAppError(e) }), []);

  const send = useCallback(
    async (kind: SendKind, modeOverride?: Mode) => {
      const s = stateRef.current;
      if (s.pending) return;
      const mode = modeOverride ?? s.mode;
      const text = kind === "message" ? s.draft.trim() : "";
      if (kind === "message" && !text) return;
      const { related: rel, excludedRelated: excluded } = relatedRef.current;
      const contextIds = rel.filter((n) => !excluded.has(n.id)).map((n) => n.id);
      const requestId = newRequestId();
      dispatch({
        type: "sendStarted",
        pending: {
          requestId,
          kind,
          mode,
          userText: text,
          reply: "",
          phase: "sending",
          model: null,
          via: null,
          retryReason: null,
          regenerate: false,
        },
      });
      setRelated([]);
      try {
        const detail = await api.sendMessage(
          {
            requestId,
            thoughtId: s.detail?.thought.id ?? null,
            text,
            mode,
            kind,
            contextIds,
            localDate: localDateLabel(),
          },
          (event) => dispatch({ type: "stream", requestId, event }),
        );
        dispatch({ type: "sendSucceeded", requestId, detail });
      } catch (e) {
        const error = toAppError(e);
        dispatch({ type: "sendFailed", requestId, error: error.kind === "cancelled" ? null : error });
      }
      focusPrimary();
    },
    [focusPrimary],
  );

  const regenerate = useCallback(async () => {
    const s = stateRef.current;
    if (s.pending || !s.detail) return;
    const thoughtId = s.detail.thought.id;
    const requestId = newRequestId();
    dispatch({
      type: "sendStarted",
      pending: {
        requestId,
        kind: "message",
        mode: s.detail.messages.at(-1)?.mode ?? s.mode,
        userText: "",
        reply: "",
        phase: "sending",
        model: null,
        via: null,
        retryReason: null,
        regenerate: true,
      },
    });
    try {
      const detail = await api.regenerate(thoughtId, requestId, (event) => dispatch({ type: "stream", requestId, event }));
      dispatch({ type: "sendSucceeded", requestId, detail });
    } catch (e) {
      const error = toAppError(e);
      dispatch({ type: "sendFailed", requestId, error: error.kind === "cancelled" ? null : error });
    }
  }, []);

  const stop = useCallback(() => {
    const p = stateRef.current.pending;
    if (p) void api.cancelMessage(p.requestId);
  }, []);

  const reloadDetail = useCallback(async (id: string) => {
    const detail = await api.getThought(id);
    dispatch({ type: "detailUpdated", detail });
  }, []);

  const flash = useCallback((text: string) => {
    dispatch({ type: "notice", text });
    window.setTimeout(() => dispatch({ type: "notice", text: null }), 2600);
  }, []);

  const extract = useCallback(
    async (what: "plan" | "tasks") => {
      const s = stateRef.current;
      if (!s.detail || s.busy) return;
      const id = s.detail.thought.id;
      dispatch({ type: "busy", busy: what });
      try {
        if (what === "plan") await api.extractPlan(id, isoDate());
        else await api.extractTasks(id, isoDate());
        await reloadDetail(id);
        flash(what === "plan" ? "Plan saved. Find it under History → Plans." : "Tasks saved. Find them under History → Tasks.");
      } catch (e) {
        fail(e);
      } finally {
        dispatch({ type: "busy", busy: null });
      }
    },
    [fail, flash, reloadDetail],
  );

  const save = useCallback(async () => {
    const s = stateRef.current;
    if (s.view !== "thought" || s.pending) return;
    if (!s.detail && s.draft.trim()) {
      dispatch({ type: "busy", busy: "capture" });
      try {
        const detail = await api.captureThought(s.draft, s.mode);
        dispatch({ type: "openThought", detail });
        flash("Saved on this Mac. Nothing was sent to Claude.");
      } catch (e) {
        fail(e);
      } finally {
        dispatch({ type: "busy", busy: null });
      }
      return;
    }
    if (!s.detail) return;
    const last = s.detail.messages.filter((m) => m.role === "assistant").at(-1);
    if (last?.mode === "plan" || (!last && s.mode === "plan")) void extract("plan");
    else if (last?.mode === "task" || (!last && s.mode === "task")) void extract("tasks");
    else flash("This thought is saved automatically.");
  }, [extract, fail, flash]);

  const openThought = useCallback(
    async (id: string) => {
      try {
        const detail = await api.getThought(id);
        dispatch({ type: "openThought", detail });
        focusPrimary();
      } catch (e) {
        fail(e);
      }
    },
    [fail, focusPrimary],
  );

  const setView = useCallback(
    (view: WidgetState["view"], tab?: ArchiveTab) => {
      dispatch({ type: "setView", view, ...(tab ? { tab } : {}) });
      focusPrimary();
    },
    [focusPrimary],
  );

  const handleAction = useCallback(
    (action: ShortcutAction) => {
      const s = stateRef.current;
      const mode = MODE_FOR_ACTION[action];
      if (mode) {
        if (s.view !== "thought") dispatch({ type: "setView", view: "thought" });
        dispatch({ type: "setMode", mode });
        const conversation = Boolean(s.detail?.messages.length);
        if (conversation && !s.draft.trim() && !s.pending && mode !== "think") void send("modeRequest", mode);
        focusPrimary();
        return;
      }
      switch (action) {
        case "submit":
          if (s.view === "thought") void send("message");
          break;
        case "close":
          if (s.view === "archive") setView("thought");
          else void api.hideWidget();
          break;
        case "stop":
          stop();
          break;
        case "clarify":
          if (s.view !== "thought") dispatch({ type: "setView", view: "thought" });
          if (s.detail?.messages.length && !s.draft.trim() && !s.pending) void send("clarify");
          else focusPrimary();
          break;
        case "history":
          setView(s.view === "archive" ? "thought" : "archive");
          break;
        case "settings":
          void api.openSettings();
          break;
        case "newThought":
          dispatch({ type: "newThought" });
          focusPrimary();
          break;
        case "save":
          void save();
          break;
      }
    },
    [focusPrimary, save, send, setView, stop],
  );

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.isComposing || e.defaultPrevented) return;
      const action = actionForEvent(e, bindingsRef.current);
      if (action) {
        e.preventDefault();
        handleAction(action);
        return;
      }
      // Keep WebKit defaults like ⌘R (reload) from disrupting the widget.
      if ((e.metaKey || e.ctrlKey) && ["KeyR", "KeyP", "KeyW"].includes(e.code)) e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [handleAction]);

  // --- Thought, plan, task, prompt edits ---------------------------------------

  const thought = state.detail?.thought ?? null;

  const header = {
    onRename: async (title: string) => {
      if (!thought) return;
      try {
        await api.renameThought(thought.id, title);
        await reloadDetail(thought.id);
      } catch (e) {
        fail(e);
      }
    },
    onTags: async (tags: string[]) => {
      if (!thought) return;
      try {
        await api.setThoughtTags(thought.id, tags);
        await reloadDetail(thought.id);
      } catch (e) {
        fail(e);
      }
    },
    onToggleMemory: async () => {
      if (!thought) return;
      try {
        await api.setThoughtMemory(thought.id, !thought.includeInMemory);
        await reloadDetail(thought.id);
        flash(
          thought.includeInMemory
            ? "This thought will no longer be offered to Claude as context."
            : "This thought can be offered to Claude as context again.",
        );
      } catch (e) {
        fail(e);
      }
    },
    onDelete: async () => {
      if (!thought) return;
      const ok = await confirmAction(
        `Delete “${thought.title}”? Its conversation, plans, tasks, and prompts are permanently removed from this Mac.`,
        "Delete thought",
        "Delete",
      );
      if (!ok) return;
      try {
        await api.deleteThought(thought.id);
        dispatch({ type: "thoughtDeleted", id: thought.id });
        flash("Deleted.");
      } catch (e) {
        fail(e);
      }
    },
  };

  const artifactHandlers = {
    onUpdatePrompt: async (prompt: Prompt, content: string) => {
      await api.updatePrompt(prompt.id, content);
      await reloadDetail(prompt.thoughtId);
    },
    onUpdatePlan: async (plan: Plan) => {
      try {
        await api.updatePlan(plan);
        if (thought) await reloadDetail(thought.id);
      } catch (e) {
        fail(e);
      }
    },
    onDeletePlan: async (plan: Plan) => {
      if (!(await confirmAction(`Delete the plan “${plan.title}”?`, "Delete plan", "Delete"))) return;
      try {
        await api.deletePlan(plan.id);
        if (thought) await reloadDetail(thought.id);
      } catch (e) {
        fail(e);
      }
    },
    onUpdateTask: async (task: Task) => {
      try {
        await api.updateTask(task);
        if (thought) await reloadDetail(thought.id);
      } catch (e) {
        fail(e);
      }
    },
    onDeleteTask: async (task: Task) => {
      try {
        await api.deleteTask(task.id);
        if (thought) await reloadDetail(thought.id);
      } catch (e) {
        fail(e);
      }
    },
  };

  // --- Render ---------------------------------------------------------------------

  const conversation = hasConversation(state);
  const usesApiKey = (settings?.claude.connection ?? "api") === "api";
  const missingKey = usesApiKey && info !== null && !info.apiKey.configured;
  const banner = bannerFor(state.error, missingKey, info);

  return (
    <div className="shell">
      <div
        ref={cardRef}
        className={`card ${motion}`}
        style={state.view === "archive" ? { height: ARCHIVE_HEIGHT } : undefined}
        onAnimationEnd={() => motion === "entering" && setMotion("")}
      >
        <Header
          thought={thought}
          view={state.view}
          bindings={bindings}
          onNew={() => handleAction("newThought")}
          onHistory={() => handleAction("history")}
          onSettings={() => void api.openSettings()}
          {...header}
        />
        {banner && (
          <div className={`banner banner-${banner.tone}`} role={banner.tone === "error" ? "alert" : "status"}>
            <span>{banner.text}</span>
            {banner.action && (
              <button className="link" onClick={() => void api.openSettings(banner.action?.section)}>
                {banner.action.label}
              </button>
            )}
            {state.error && (
              <button className="banner-close" aria-label="Dismiss" onClick={() => dispatch({ type: "error", error: null })}>
                ×
              </button>
            )}
          </div>
        )}
        {state.view === "archive" ? (
          <Archive
            tab={state.archiveTab}
            searchRef={searchRef}
            bindings={bindings}
            openThoughtId={thought?.id ?? null}
            onTab={(tab) => dispatch({ type: "setView", view: "archive", tab })}
            onOpen={(id) => void openThought(id)}
            onDeleted={(id) => dispatch({ type: "thoughtDeleted", id })}
            onError={(error: AppError) => dispatch({ type: "error", error })}
          />
        ) : (
          <>
            {conversation && (
              <ConversationView
                detail={state.detail}
                pending={state.pending}
                busy={state.busy}
                bindings={bindings}
                maxHeight={conversationMaxHeight()}
                onRegenerate={() => void regenerate()}
                onSavePlan={() => void extract("plan")}
                onSaveTasks={() => void extract("tasks")}
                onCopy={(text) => void copyText(text).then(() => flash("Copied."))}
                {...artifactHandlers}
              />
            )}
            {!conversation && state.detail && (
              <div className="captured">
                <p className="label">Saved thought</p>
                <p className="captured-text">{state.detail.thought.rawInput}</p>
                <button
                  className="btn"
                  onClick={() => {
                    dispatch({ type: "setDraft", text: state.detail?.thought.rawInput ?? "" });
                    focusPrimary();
                  }}
                >
                  Think it through with Claude
                </button>
              </div>
            )}
            <Composer
              inputRef={composerRef}
              empty={!conversation && !state.detail}
              draft={state.draft}
              mode={state.mode}
              sending={Boolean(state.pending)}
              bindings={bindings}
              related={related}
              excludedRelated={excludedRelated}
              canSend={Boolean(state.draft.trim()) && !state.pending}
              onDraft={(text) => dispatch({ type: "setDraft", text })}
              onMode={(mode) => handleAction(Object.entries(MODE_FOR_ACTION).find(([, m]) => m === mode)?.[0] as ShortcutAction)}
              onSubmit={() => void send("message")}
              onStop={stop}
              onToggleRelated={(id) =>
                setExcludedRelated((prev) => {
                  const next = new Set(prev);
                  if (next.has(id)) next.delete(id);
                  else next.add(id);
                  return next;
                })
              }
            />
          </>
        )}
        {state.notice && (
          <div className="notice" role="status">
            {state.notice}
          </div>
        )}
      </div>
    </div>
  );
}

interface Banner {
  tone: "error" | "info";
  text: string;
  action?: { label: string; section: string };
}

function bannerFor(error: AppError | null, missingKey: boolean, info: AppInfo | null): Banner | null {
  if (error) {
    const keyProblem = [
      "missingApiKey",
      "invalidApiKey",
      "modelNotFound",
      "billing",
      "permissionDenied",
      "claudeCodeMissing",
      "claudeCodeSignedOut",
    ].includes(error.kind);
    return {
      tone: "error",
      text: error.message,
      ...(keyProblem ? { action: { label: "Open Claude settings", section: "claude" } } : {}),
    };
  }
  if (info?.startupWarning) return { tone: "info", text: info.startupWarning };
  if (info && !info.shortcut.registered && info.shortcut.error) {
    return { tone: "info", text: info.shortcut.error, action: { label: "Change shortcut", section: "general" } };
  }
  if (missingKey) {
    return {
      tone: "info",
      text: "Add an Anthropic API key, or connect Claude Code, to think with Claude. You can still save thoughts locally.",
      action: { label: "Set up Claude", section: "claude" },
    };
  }
  return null;
}
