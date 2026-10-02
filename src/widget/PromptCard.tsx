import { useEffect, useRef, useState } from "react";
import { Kbd } from "../components/Kbd";
import { copyText } from "../lib/api";

interface Props {
  content: string;
  complete: boolean;
  edited: boolean;
  /** Only the newest prompt can be regenerated (it replaces the latest reply). */
  canRegenerate: boolean;
  busy: boolean;
  onRegenerate: () => void;
  onSave: ((content: string) => Promise<void>) | null;
}

/** The "Generated Prompt" panel shown in Prompt Mode. */
export function PromptCard({ content, complete, edited, canRegenerate, busy, onRegenerate, onSave }: Props) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(content);
  const [copied, setCopied] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const editorRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    if (!editing) setDraft(content);
  }, [content, editing]);

  useEffect(() => {
    const el = editorRef.current;
    if (!editing || !el) return;
    el.focus();
    el.setSelectionRange(el.value.length, el.value.length);
  }, [editing]);

  useEffect(() => {
    if (!copied) return;
    const t = window.setTimeout(() => setCopied(false), 1600);
    return () => window.clearTimeout(t);
  }, [copied]);

  const copy = async () => {
    await copyText(editing ? draft : content);
    setCopied(true);
  };

  const save = async () => {
    if (!onSave) return;
    try {
      await onSave(draft);
      setEditing(false);
      setSaveError(null);
    } catch (e) {
      setSaveError((e as { message?: string }).message ?? "Couldn't save the prompt.");
    }
  };

  return (
    <section className="prompt-card" aria-label="Generated prompt">
      <header className="prompt-card-head">
        <span className="label">
          Generated prompt{edited ? " · edited" : ""}
          {!complete && " · writing…"}
        </span>
        <span className="prompt-card-actions">
          {complete && onSave && !editing && (
            <button className="btn btn-quiet" onClick={() => setEditing(true)}>
              Edit
            </button>
          )}
          {complete && canRegenerate && !editing && (
            <button className="btn btn-quiet" onClick={onRegenerate} disabled={busy}>
              Regenerate
            </button>
          )}
          {editing && (
            <>
              <button className="btn btn-quiet" onClick={() => setEditing(false)}>
                Cancel
              </button>
              <button className="btn" onClick={() => void save()}>
                Save <Kbd keys="CmdOrCtrl+Enter" />
              </button>
            </>
          )}
          <button className="btn btn-primary" onClick={() => void copy()} disabled={!complete && !content}>
            {copied ? "Copied" : "Copy Prompt"}
          </button>
        </span>
      </header>
      {editing ? (
        <textarea
          ref={editorRef}
          className="prompt-editor"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          aria-label="Edit generated prompt"
          onKeyDown={(e) => {
            if (e.key === "Escape") {
              e.stopPropagation();
              setEditing(false);
            } else if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
              e.preventDefault();
              e.stopPropagation();
              void save();
            }
          }}
        />
      ) : (
        <pre className="prompt-body" tabIndex={0}>
          {content}
        </pre>
      )}
      {saveError && <p className="inline-error">{saveError}</p>}
    </section>
  );
}
