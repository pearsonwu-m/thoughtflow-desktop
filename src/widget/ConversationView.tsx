import { useEffect, useLayoutEffect, useRef, type ReactNode } from "react";
import { Kbd } from "../components/Kbd";
import { Markdown } from "../components/Markdown";
import { formatTime, MODE_LABELS, modelLabel } from "../lib/format";
import { splitPromptBlocks } from "../lib/markdown";
import type { Busy, PendingTurn } from "../state/widget";
import type { Message, Mode, Plan, Prompt, ShortcutAction, Task, ThoughtDetail } from "../types";
import { PlanCard } from "./PlanCard";
import { PromptCard } from "./PromptCard";
import { TaskList } from "./TaskList";

interface Props {
  detail: ThoughtDetail | null;
  pending: PendingTurn | null;
  busy: Busy;
  bindings: Record<ShortcutAction, string>;
  maxHeight: number;
  onRegenerate: () => void;
  onSavePlan: () => void;
  onSaveTasks: () => void;
  onCopy: (text: string) => void;
  onUpdatePrompt: (prompt: Prompt, content: string) => Promise<void>;
  onUpdatePlan: (plan: Plan) => Promise<void>;
  onDeletePlan: (plan: Plan) => Promise<void>;
  onUpdateTask: (task: Task) => Promise<void>;
  onDeleteTask: (task: Task) => Promise<void>;
}

export function cueText(kind: Message["kind"], mode: Mode): string {
  if (kind === "clarify") return "Asked for one clarifying question";
  switch (mode) {
    case "plan":
      return "Asked for a plan";
    case "task":
      return "Asked for the tasks";
    case "reflect":
      return "Asked for a reflection";
    case "prompt":
      return "Asked for a prompt";
    default:
      return "Asked to think it through";
  }
}

function UserTurn({ text, time, contextCount }: { text: string; time: number; contextCount: number }) {
  return (
    <div className="turn turn-user">
      <time className="gutter">{formatTime(time)}</time>
      <div>
        <p className="user-text">{text}</p>
        {contextCount > 0 && (
          <p className="turn-note">
            with {contextCount} related note{contextCount === 1 ? "" : "s"}
          </p>
        )}
      </div>
    </div>
  );
}

function CueTurn({ text, time }: { text: string; time: number }) {
  return (
    <div className="turn turn-cue">
      <time className="gutter">{formatTime(time)}</time>
      <p>→ {text}</p>
    </div>
  );
}

function ReplyBody({
  text,
  prompts,
  messageId,
  streaming,
  canRegenerate,
  busy,
  onRegenerate,
  onUpdatePrompt,
}: {
  text: string;
  prompts: Prompt[];
  messageId: string | null;
  streaming: boolean;
  canRegenerate: boolean;
  busy: boolean;
  onRegenerate: () => void;
  onUpdatePrompt: Props["onUpdatePrompt"];
}) {
  const segments = splitPromptBlocks(text);
  const record = messageId ? prompts.find((p) => p.messageId === messageId) : undefined;
  return (
    <>
      {segments.map((seg, i) =>
        seg.type === "text" ? (
          <Markdown key={i} text={seg.text} />
        ) : (
          <PromptCard
            key={i}
            content={record?.edited ? record.content : seg.text}
            complete={seg.complete && !streaming}
            edited={Boolean(record?.edited)}
            canRegenerate={canRegenerate}
            busy={busy}
            onRegenerate={onRegenerate}
            onSave={record ? (content) => onUpdatePrompt(record, content) : null}
          />
        ),
      )}
    </>
  );
}

const PHASE_TEXT: Record<PendingTurn["phase"], string> = {
  sending: "Sending to Claude",
  thinking: "Claude is thinking",
  writing: "",
  retrying: "Claude is busy; retrying",
};

/** The thought, read top to bottom like a notebook page. */
export function ConversationView(props: Props) {
  const { detail, pending, busy, bindings, maxHeight } = props;
  const scrollRef = useRef<HTMLDivElement>(null);
  const pinnedToBottom = useRef(true);
  const messages = detail?.messages ?? [];
  const lastAssistant = [...messages].reverse().find((m) => m.role === "assistant");

  // Follow new content unless the user has scrolled up to read.
  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (el && pinnedToBottom.current) el.scrollTop = el.scrollHeight;
  });

  useEffect(() => {
    pinnedToBottom.current = true;
  }, [detail?.thought.id]);

  const rows: ReactNode[] = [];
  messages.forEach((m, index) => {
    const replaced = pending?.regenerate && m.id === lastAssistant?.id;
    if (replaced) return;
    if (m.role === "system") return;
    if (m.role === "user") {
      // A mode switch is stored right after the user turn that caused it;
      // show it as a section divider above that turn.
      const next = messages[index + 1];
      if (next?.role === "system" && (index > 0 || next.mode !== "think")) {
        rows.push(
          <div key={next.id} className="mode-divider" role="separator">
            <span className="label">{MODE_LABELS[next.mode]}</span>
          </div>,
        );
      }
      rows.push(
        m.kind === "message" ? (
          <UserTurn key={m.id} text={m.text} time={m.createdAt} contextCount={m.contextIds.length} />
        ) : (
          <CueTurn key={m.id} text={cueText(m.kind, m.mode)} time={m.createdAt} />
        ),
      );
      return;
    }
    const isLast = m.id === lastAssistant?.id && !pending;
    rows.push(
      <div key={m.id} className="turn turn-claude">
        <span className="gutter" aria-hidden="true" />
        <div className="reply">
          <ReplyBody
            text={m.text}
            prompts={detail?.prompts ?? []}
            messageId={m.id}
            streaming={false}
            canRegenerate={isLast}
            busy={Boolean(pending)}
            onRegenerate={props.onRegenerate}
            onUpdatePrompt={props.onUpdatePrompt}
          />
          {m.truncated && <p className="turn-note">This reply hit the length limit and was cut short.</p>}
          {isLast && (
            <div className="reply-actions">
              {m.mode === "plan" && (
                <button className="btn" onClick={props.onSavePlan} disabled={busy !== null}>
                  {busy === "plan" ? "Saving plan…" : "Save plan"} <Kbd keys={bindings.save} />
                </button>
              )}
              {m.mode === "task" && (
                <button className="btn" onClick={props.onSaveTasks} disabled={busy !== null}>
                  {busy === "tasks" ? "Saving tasks…" : "Save tasks"} <Kbd keys={bindings.save} />
                </button>
              )}
              <button className="btn btn-quiet" onClick={() => props.onCopy(m.text)}>
                Copy
              </button>
              <button className="btn btn-quiet" onClick={props.onRegenerate}>
                Regenerate
              </button>
              {m.model && <span className="reply-model">{modelLabel(m.model)}</span>}
            </div>
          )}
        </div>
      </div>,
    );
  });

  if (pending) {
    if (!pending.regenerate) {
      const now = Date.now();
      const switching = detail?.messages.length ? detail.thought.mode !== pending.mode : pending.mode !== "think";
      if (switching) {
        rows.push(
          <div key="pending-divider" className="mode-divider" role="separator">
            <span className="label">{MODE_LABELS[pending.mode]}</span>
          </div>,
        );
      }
      rows.push(
        pending.kind === "message" ? (
          <UserTurn key="pending-user" text={pending.userText} time={now} contextCount={0} />
        ) : (
          <CueTurn key="pending-user" text={cueText(pending.kind, pending.mode)} time={now} />
        ),
      );
    }
    rows.push(
      <div key="pending-reply" className="turn turn-claude" aria-live="polite">
        <span className="gutter" aria-hidden="true" />
        <div className="reply">
          {pending.reply ? (
            <ReplyBody
              text={pending.reply}
              prompts={[]}
              messageId={null}
              streaming
              canRegenerate={false}
              busy
              onRegenerate={props.onRegenerate}
              onUpdatePrompt={props.onUpdatePrompt}
            />
          ) : null}
          <p className={`status-line${pending.phase === "writing" ? " status-writing" : ""}`}>
            <span className="pulse" aria-hidden="true" />
            {PHASE_TEXT[pending.phase] || "Writing"}
            {pending.phase === "sending" && pending.via === "claude-code" ? " via Claude Code" : ""}
            {pending.model && pending.phase !== "writing" ? ` · ${modelLabel(pending.model)}` : ""}
            {pending.phase !== "writing" && "…"}
            <span className="status-hint">
              <Kbd keys={bindings.stop} /> stop
            </span>
          </p>
        </div>
      </div>,
    );
  }

  const plans = detail?.plans ?? [];
  const tasks = detail?.tasks ?? [];

  return (
    <div
      className="conversation"
      ref={scrollRef}
      style={{ maxHeight }}
      onScroll={(e) => {
        const el = e.currentTarget;
        pinnedToBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
      }}
    >
      {rows}
      {plans.length > 0 && (
        <section className="artifacts" aria-label="Saved plans">
          <h2 className="label">Saved plan{plans.length > 1 ? "s" : ""}</h2>
          {plans.map((p) => (
            <PlanCard key={p.id} plan={p} compact onChange={props.onUpdatePlan} onDelete={props.onDeletePlan} />
          ))}
        </section>
      )}
      {tasks.length > 0 && (
        <section className="artifacts" aria-label="Saved tasks">
          <h2 className="label">Tasks</h2>
          <TaskList tasks={tasks} onChange={props.onUpdateTask} onDelete={props.onDeleteTask} />
        </section>
      )}
    </div>
  );
}
