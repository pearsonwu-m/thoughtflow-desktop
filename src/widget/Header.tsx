import { useEffect, useRef, useState } from "react";
import { Kbd } from "../components/Kbd";
import type { View } from "../state/widget";
import type { ShortcutAction, Thought } from "../types";

interface Props {
  thought: Thought | null;
  view: View;
  bindings: Record<ShortcutAction, string>;
  onNew: () => void;
  onHistory: () => void;
  onSettings: () => void;
  onRename: (title: string) => Promise<void>;
  onTags: (tags: string[]) => Promise<void>;
  onToggleMemory: () => Promise<void>;
  onDelete: () => Promise<void>;
}

type Editing = null | "title" | "tags";

/** Title bar: brand, the open thought, and navigation. Doubles as the drag handle. */
export function Header(props: Props) {
  const { thought, view, bindings } = props;
  const [editing, setEditing] = useState<Editing>(null);
  const [value, setValue] = useState("");
  const [menuOpen, setMenuOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);
  const menuButtonRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    setEditing(null);
    setMenuOpen(false);
  }, [thought?.id]);

  useEffect(() => {
    if (menuOpen) menuRef.current?.querySelector<HTMLButtonElement>("button")?.focus();
  }, [menuOpen]);

  const begin = (what: Exclude<Editing, null>) => {
    if (!thought) return;
    setValue(what === "title" ? thought.title : thought.tags.join(", "));
    setEditing(what);
    setMenuOpen(false);
  };

  const commit = async () => {
    if (editing === "title" && value.trim()) await props.onRename(value.trim());
    if (editing === "tags") {
      await props.onTags(
        value
          .split(/[,\s]+/)
          .map((t) => t.trim())
          .filter(Boolean),
      );
    }
    setEditing(null);
  };

  const closeMenu = () => {
    setMenuOpen(false);
    menuButtonRef.current?.focus();
  };

  const run = (fn: () => unknown) => () => {
    setMenuOpen(false);
    void fn();
  };

  return (
    <header className="header" data-tauri-drag-region>
      <span className="brand" data-tauri-drag-region>
        Thoughtflow
      </span>
      {view === "thought" && thought && editing === null && (
        <button className="header-title" onClick={() => begin("title")} title="Rename">
          {thought.title}
          {thought.tags.length > 0 && <span className="header-tags">{thought.tags.map((t) => `#${t}`).join(" ")}</span>}
        </button>
      )}
      {view === "archive" && (
        <span className="header-title header-static" data-tauri-drag-region>
          Archive
        </span>
      )}
      {editing && (
        <input
          className="header-input"
          autoFocus
          value={value}
          placeholder={editing === "tags" ? "tags, separated by commas" : "Title"}
          aria-label={editing === "tags" ? "Tags" : "Title"}
          onChange={(e) => setValue(e.target.value)}
          onBlur={() => setEditing(null)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              void commit();
            } else if (e.key === "Escape") {
              e.stopPropagation();
              setEditing(null);
            }
          }}
        />
      )}
      <span className="header-fill" data-tauri-drag-region />
      <nav className="header-actions">
        <button className="hbtn" onClick={props.onNew} title="New thought">
          New <Kbd keys={bindings.newThought} />
        </button>
        <button className={`hbtn${view === "archive" ? " hbtn-on" : ""}`} onClick={props.onHistory} title="Thought history">
          History <Kbd keys={bindings.history} />
        </button>
        <button className="hbtn" onClick={props.onSettings} title="Settings">
          Settings <Kbd keys={bindings.settings} />
        </button>
        {view === "thought" && thought && (
          <span className="menu-anchor">
            <button
              ref={menuButtonRef}
              className="hbtn hbtn-icon"
              aria-haspopup="menu"
              aria-expanded={menuOpen}
              aria-label="Thought options"
              onClick={() => setMenuOpen((o) => !o)}
            >
              ⋯
            </button>
            {menuOpen && (
              <div
                ref={menuRef}
                className="menu"
                role="menu"
                onKeyDown={(e) => {
                  const items = Array.from(menuRef.current?.querySelectorAll<HTMLButtonElement>("button") ?? []);
                  const i = items.indexOf(document.activeElement as HTMLButtonElement);
                  if (e.key === "Escape") {
                    e.stopPropagation();
                    closeMenu();
                  } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
                    e.preventDefault();
                    const next = items[(i + (e.key === "ArrowDown" ? 1 : items.length - 1)) % items.length];
                    next?.focus();
                  }
                }}
                onBlur={(e) => {
                  if (!menuRef.current?.contains(e.relatedTarget as Node)) setMenuOpen(false);
                }}
              >
                <button role="menuitem" onClick={() => begin("title")}>
                  Rename
                </button>
                <button role="menuitem" onClick={() => begin("tags")}>
                  Edit tags
                </button>
                <button role="menuitem" onClick={run(props.onToggleMemory)}>
                  {thought.includeInMemory ? "Keep out of Claude's memory" : "Allow in Claude's memory"}
                </button>
                <button role="menuitem" className="menu-danger" onClick={run(props.onDelete)}>
                  Delete permanently…
                </button>
              </div>
            )}
          </span>
        )}
      </nav>
    </header>
  );
}
