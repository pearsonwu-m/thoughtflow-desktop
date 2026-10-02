import { useLayoutEffect, type RefObject } from "react";
import { Kbd } from "../components/Kbd";
import { MODE_LABELS } from "../lib/format";
import { MODES, type Mode, type RelatedNote, type ShortcutAction } from "../types";

const MODE_ACTION: Record<Mode, ShortcutAction> = {
  think: "thinkMode",
  plan: "planMode",
  task: "taskMode",
  reflect: "reflectMode",
  prompt: "promptMode",
};

const PLACEHOLDERS: Record<Mode, string> = {
  think: "Reply, or keep thinking out loud…",
  plan: "Anything the plan should account for?",
  task: "Anything to add before pulling out tasks?",
  reflect: "What do you want to look at?",
  prompt: "Answer, or say “just generate it”…",
};

interface Props {
  inputRef: RefObject<HTMLTextAreaElement | null>;
  empty: boolean;
  draft: string;
  mode: Mode;
  sending: boolean;
  bindings: Record<ShortcutAction, string>;
  related: RelatedNote[];
  excludedRelated: Set<string>;
  canSend: boolean;
  onDraft: (text: string) => void;
  onMode: (mode: Mode) => void;
  onSubmit: () => void;
  onStop: () => void;
  onToggleRelated: (id: string) => void;
}

export function Composer(props: Props) {
  const { inputRef, empty, draft, mode, sending, bindings, related, excludedRelated } = props;

  // Grow with the text, up to a limit; the conversation keeps the rest of the space.
  useLayoutEffect(() => {
    const el = inputRef.current;
    if (!el) return;
    el.style.height = "0px";
    el.style.height = `${Math.min(el.scrollHeight, empty ? 220 : 150)}px`;
  }, [draft, empty, inputRef]);

  const included = related.filter((n) => !excludedRelated.has(n.id)).length;

  return (
    <div className={`composer${empty ? " composer-empty" : ""}`}>
      {empty && (
        <label className="prompt-heading" htmlFor="tf-composer">
          What are you thinking about?
        </label>
      )}
      <textarea
        id="tf-composer"
        ref={inputRef}
        className="composer-input"
        value={draft}
        rows={empty ? 3 : 1}
        spellCheck
        placeholder={empty ? "" : PLACEHOLDERS[mode]}
        aria-label={empty ? undefined : "Reply"}
        onChange={(e) => props.onDraft(e.target.value)}
      />
      {related.length > 0 && (
        <div className="related" aria-label="Related notes">
          <span className="related-label">
            {included ? `Sending ${included} related note${included === 1 ? "" : "s"} as context` : "Related notes"}
          </span>
          {related.map((note) => {
            const on = !excludedRelated.has(note.id);
            return (
              <button
                key={note.id}
                className={`chip${on ? " chip-on" : ""}`}
                aria-pressed={on}
                title={note.snippet}
                onClick={() => props.onToggleRelated(note.id)}
              >
                {note.title}
              </button>
            );
          })}
        </div>
      )}
      <div className="composer-footer">
        <div className="modes" role="radiogroup" aria-label="Mode">
          {MODES.map((m) => (
            <button
              key={m}
              role="radio"
              aria-checked={m === mode}
              className={`mode${m === mode ? " mode-on" : ""}`}
              onClick={() => props.onMode(m)}
              title={`${MODE_LABELS[m]} mode`}
            >
              {MODE_LABELS[m]}
              {m !== mode && <Kbd keys={bindings[MODE_ACTION[m]]} />}
            </button>
          ))}
        </div>
        <span className="spacer" />
        {sending ? (
          <button className="btn" onClick={props.onStop}>
            Stop <Kbd keys={bindings.stop} />
          </button>
        ) : (
          <button className="btn btn-primary" onClick={props.onSubmit} disabled={!props.canSend}>
            Send <Kbd keys={bindings.submit} />
          </button>
        )}
      </div>
      {empty && (
        <div className="hints">
          <span>
            <Kbd keys={bindings.submit} /> send to Claude
          </span>
          <span>
            <Kbd keys={bindings.save} /> save without sending
          </span>
          <span>
            <Kbd keys={bindings.history} /> history
          </span>
          <span>
            <Kbd keys={bindings.close} /> close
          </span>
        </div>
      )}
    </div>
  );
}
