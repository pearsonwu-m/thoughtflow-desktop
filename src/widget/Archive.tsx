import { useCallback, useEffect, useMemo, useRef, useState, type RefObject } from "react";
import { Kbd } from "../components/Kbd";
import { api, confirmAction, toAppError } from "../lib/api";
import { formatTime, groupByDay, MODE_LABELS } from "../lib/format";
import type { ArchiveTab } from "../state/widget";
import type { AppError, Plan, ShortcutAction, Task, Thought } from "../types";
import { PlanCard } from "./PlanCard";
import { TaskList } from "./TaskList";

interface Props {
  tab: ArchiveTab;
  searchRef: RefObject<HTMLInputElement | null>;
  bindings: Record<ShortcutAction, string>;
  openThoughtId: string | null;
  onTab: (tab: ArchiveTab) => void;
  onOpen: (id: string) => void;
  onDeleted: (id: string) => void;
  onError: (error: AppError) => void;
}

const TABS: { id: ArchiveTab; label: string; key: string }[] = [
  { id: "thoughts", label: "Thoughts", key: "CmdOrCtrl+1" },
  { id: "plans", label: "Plans", key: "CmdOrCtrl+2" },
  { id: "tasks", label: "Tasks", key: "CmdOrCtrl+3" },
];

const PLACEHOLDER: Record<ArchiveTab, string> = {
  thoughts: "Search thoughts",
  plans: "Search plans",
  tasks: "Search tasks",
};

/** History as a research notebook: thoughts by day, saved plans, and tasks. */
export function Archive(props: Props) {
  const { tab, searchRef, onError, onTab } = props;
  const [query, setQuery] = useState("");

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey) || e.shiftKey || e.altKey) return;
      const index = ["Digit1", "Digit2", "Digit3"].indexOf(e.code);
      const target = TABS[index];
      if (target) {
        e.preventDefault();
        onTab(target.id);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onTab]);

  return (
    <div className="archive">
      <div className="archive-top">
        <input
          ref={searchRef}
          className="search"
          type="text"
          spellCheck={false}
          value={query}
          placeholder={PLACEHOLDER[tab]}
          aria-label={PLACEHOLDER[tab]}
          onChange={(e) => setQuery(e.target.value)}
          autoFocus
        />
        <div className="tabs" role="tablist" aria-label="Archive sections">
          {TABS.map((t) => (
            <button
              key={t.id}
              role="tab"
              aria-selected={tab === t.id}
              className={`tab${tab === t.id ? " tab-on" : ""}`}
              onClick={() => onTab(t.id)}
            >
              {t.label}
              <Kbd keys={t.key} />
            </button>
          ))}
        </div>
      </div>
      {tab === "thoughts" && <ThoughtList {...props} query={query} />}
      {tab === "plans" && <PlanList query={query} onError={onError} onOpen={props.onOpen} />}
      {tab === "tasks" && <TaskArchive query={query} onError={onError} />}
    </div>
  );
}

function ThoughtList({
  query,
  searchRef,
  openThoughtId,
  onOpen,
  onDeleted,
  onError,
}: Props & { query: string }) {
  const [thoughts, setThoughts] = useState<Thought[] | null>(null);
  const [selected, setSelected] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);

  const load = useCallback(
    (q: string) =>
      api
        .listThoughts(q)
        .then((list) => {
          setThoughts(list);
          setSelected(0);
        })
        .catch((e) => onError(toAppError(e))),
    [onError],
  );

  useEffect(() => {
    const t = window.setTimeout(() => void load(query), query ? 120 : 0);
    return () => window.clearTimeout(t);
  }, [query, load]);

  const groups = useMemo(() => groupByDay(thoughts ?? []), [thoughts]);

  const remove = useCallback(
    async (thought: Thought) => {
      const ok = await confirmAction(
        `Delete “${thought.title}”? Its conversation, plans, tasks, and prompts are permanently removed from this Mac.`,
        "Delete thought",
        "Delete",
      );
      if (!ok) return;
      try {
        await api.deleteThought(thought.id);
        onDeleted(thought.id);
        await load(query);
      } catch (e) {
        onError(toAppError(e));
      }
    },
    [load, onDeleted, onError, query],
  );

  // Arrow keys move through the list while typing in the search box.
  useEffect(() => {
    const input = searchRef.current;
    if (!input || !thoughts) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        setSelected((i) => Math.max(0, Math.min(thoughts.length - 1, i + (e.key === "ArrowDown" ? 1 : -1))));
      } else if (e.key === "Enter" && !e.metaKey) {
        const t = thoughts[selected];
        if (t) {
          e.preventDefault();
          onOpen(t.id);
        }
      } else if (e.key === "Backspace" && (e.metaKey || e.ctrlKey)) {
        const t = thoughts[selected];
        if (t) {
          e.preventDefault();
          void remove(t);
        }
      }
    };
    input.addEventListener("keydown", onKey);
    return () => input.removeEventListener("keydown", onKey);
  }, [searchRef, thoughts, selected, onOpen, remove]);

  useEffect(() => {
    listRef.current?.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: "nearest" });
  }, [selected]);

  if (thoughts === null) return <div className="archive-list" />;
  if (!thoughts.length) {
    return (
      <div className="archive-list">
        <p className="empty-note">{query ? "Nothing matches that search." : "Saved thoughts will appear here."}</p>
      </div>
    );
  }

  let index = -1;
  return (
    <>
      <div className="archive-list" ref={listRef} role="listbox" aria-label="Thoughts">
        {groups.map((group) => (
          <section key={group.label}>
            <h3 className="day">{group.label}</h3>
            {group.items.map((t) => {
              index += 1;
              const i = index;
              return (
                <div
                  key={t.id}
                  role="option"
                  aria-selected={i === selected}
                  className={`row${t.id === openThoughtId ? " row-open" : ""}`}
                  onMouseMove={() => setSelected(i)}
                  onClick={() => onOpen(t.id)}
                >
                  <time>{formatTime(t.updatedAt)}</time>
                  <span className="row-title">
                    {t.title}
                    {t.tags.length > 0 && <span className="row-tags"> {t.tags.map((x) => `#${x}`).join(" ")}</span>}
                  </span>
                  <span className="row-meta">
                    {!t.includeInMemory && <span title="Excluded from Claude's memory">private · </span>}
                    {MODE_LABELS[t.mode].toLowerCase()}
                  </span>
                </div>
              );
            })}
          </section>
        ))}
      </div>
      <div className="archive-foot">
        <span>
          <Kbd keys="Up" />
          <Kbd keys="Down" /> move
        </span>
        <span>
          <Kbd keys="Enter" /> open
        </span>
        <span>
          <Kbd keys="CmdOrCtrl+Backspace" /> delete
        </span>
        <span>
          <Kbd keys="Escape" /> back
        </span>
      </div>
    </>
  );
}

function matches(text: string, query: string): boolean {
  return text.toLowerCase().includes(query.trim().toLowerCase());
}

function PlanList({ query, onError, onOpen }: { query: string; onError: (e: AppError) => void; onOpen: (id: string) => void }) {
  const [plans, setPlans] = useState<Plan[] | null>(null);
  const [expanded, setExpanded] = useState<string | null>(null);

  const load = useCallback(() => api.listPlans().then(setPlans).catch((e) => onError(toAppError(e))), [onError]);
  useEffect(() => void load(), [load]);

  const visible = (plans ?? []).filter(
    (p) => !query.trim() || matches(`${p.title} ${p.objective} ${p.steps.map((s) => s.title).join(" ")}`, query),
  );

  const update = async (plan: Plan) => {
    try {
      const saved = await api.updatePlan(plan);
      setPlans((list) => list?.map((p) => (p.id === saved.id ? saved : p)) ?? null);
    } catch (e) {
      onError(toAppError(e));
    }
  };
  const remove = async (plan: Plan) => {
    if (!(await confirmAction(`Delete the plan “${plan.title}”?`, "Delete plan", "Delete"))) return;
    try {
      await api.deletePlan(plan.id);
      setPlans((list) => list?.filter((p) => p.id !== plan.id) ?? null);
    } catch (e) {
      onError(toAppError(e));
    }
  };

  if (plans === null) return <div className="archive-list" />;
  if (!visible.length) {
    return (
      <div className="archive-list">
        <p className="empty-note">
          {query ? "No plans match." : "Plans you save in Plan mode (⌘P, then ⌘S) appear here."}
        </p>
      </div>
    );
  }
  return (
    <div className="archive-list">
      {visible.map((p) => (
        <div key={p.id} className="plan-row">
          <button
            className="row"
            aria-expanded={expanded === p.id}
            onClick={() => setExpanded(expanded === p.id ? null : p.id)}
          >
            <span className={`status-dot status-${p.status}`} aria-label={p.status} />
            <span className="row-title">{p.title}</span>
            <span className="row-meta">
              {p.steps.filter((s) => s.done).length}/{p.steps.length} steps
            </span>
          </button>
          {expanded === p.id && (
            <div className="plan-expanded">
              <PlanCard plan={p} onChange={update} onDelete={remove} />
              {p.thoughtId && (
                <button className="link" onClick={() => onOpen(p.thoughtId as string)}>
                  Open the original thought
                </button>
              )}
            </div>
          )}
        </div>
      ))}
    </div>
  );
}

function TaskArchive({ query, onError }: { query: string; onError: (e: AppError) => void }) {
  const [tasks, setTasks] = useState<Task[] | null>(null);
  const [newName, setNewName] = useState("");

  const load = useCallback(() => api.listTasks().then(setTasks).catch((e) => onError(toAppError(e))), [onError]);
  useEffect(() => void load(), [load]);

  const visible = (tasks ?? []).filter((t) => !query.trim() || matches(`${t.name} ${t.description}`, query));
  const open = visible.filter((t) => !t.completed);
  const done = visible.filter((t) => t.completed);

  const update = async (task: Task) => {
    try {
      await api.updateTask(task);
      await load();
    } catch (e) {
      onError(toAppError(e));
    }
  };
  const remove = async (task: Task) => {
    try {
      await api.deleteTask(task.id);
      setTasks((list) => list?.filter((t) => t.id !== task.id) ?? null);
    } catch (e) {
      onError(toAppError(e));
    }
  };
  const add = async () => {
    if (!newName.trim()) return;
    try {
      await api.createTask({ name: newName.trim() });
      setNewName("");
      await load();
    } catch (e) {
      onError(toAppError(e));
    }
  };

  return (
    <div className="archive-list">
      <input
        className="task-add"
        value={newName}
        placeholder="Add a task and press ↵"
        aria-label="Add a task"
        onChange={(e) => setNewName(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            void add();
          }
        }}
      />
      {tasks !== null && !visible.length && (
        <p className="empty-note">{query ? "No tasks match." : "Tasks you save in Task mode (⌘T, then ⌘S) appear here."}</p>
      )}
      <TaskList tasks={open} onChange={update} onDelete={remove} />
      {done.length > 0 && (
        <>
          <h3 className="day">Completed</h3>
          <TaskList tasks={done} onChange={update} onDelete={remove} />
        </>
      )}
    </div>
  );
}
