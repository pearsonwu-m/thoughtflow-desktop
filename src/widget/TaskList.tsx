import { useState } from "react";
import { EFFORT_LABELS, formatDue } from "../lib/format";
import type { Priority, Task, TaskEffort } from "../types";

interface Props {
  tasks: Task[];
  onChange: (task: Task) => Promise<void>;
  onDelete: (task: Task) => Promise<void>;
}

const PRIORITIES: Priority[] = ["high", "medium", "low"];
const EFFORTS: TaskEffort[] = ["quick", "short", "medium", "long"];

function TaskRow({ task, onChange, onDelete }: { task: Task } & Omit<Props, "tasks">) {
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState(task.name);
  const [description, setDescription] = useState(task.description);
  const [priority, setPriority] = useState<Priority>(task.priority);
  const [effort, setEffort] = useState<TaskEffort>(task.effort);
  const [due, setDue] = useState(task.dueDate ?? "");

  const startEdit = () => {
    setName(task.name);
    setDescription(task.description);
    setPriority(task.priority);
    setEffort(task.effort);
    setDue(task.dueDate ?? "");
    setEditing(true);
  };

  const commit = async () => {
    if (!name.trim()) return;
    await onChange({ ...task, name: name.trim(), description: description.trim(), priority, effort, dueDate: due || null });
    setEditing(false);
  };

  if (editing) {
    return (
      <li className="task task-editing">
        <div
          className="task-edit"
          onKeyDown={(e) => {
            if (e.key === "Escape") {
              e.stopPropagation();
              setEditing(false);
            } else if (e.key === "Enter" && !e.shiftKey && (e.target as HTMLElement).tagName === "INPUT") {
              e.preventDefault();
              e.stopPropagation();
              void commit();
            }
          }}
        >
          <input autoFocus value={name} onChange={(e) => setName(e.target.value)} aria-label="Task name" />
          <input
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            placeholder="Description (optional)"
            aria-label="Task description"
          />
          <div className="task-edit-row">
            <select value={priority} onChange={(e) => setPriority(e.target.value as Priority)} aria-label="Priority">
              {PRIORITIES.map((p) => (
                <option key={p} value={p}>
                  {p} priority
                </option>
              ))}
            </select>
            <select value={effort} onChange={(e) => setEffort(e.target.value as TaskEffort)} aria-label="Effort">
              {EFFORTS.map((x) => (
                <option key={x} value={x}>
                  {EFFORT_LABELS[x]}
                </option>
              ))}
            </select>
            <input type="date" value={due} onChange={(e) => setDue(e.target.value)} aria-label="Due date" />
            <span className="spacer" />
            <button className="btn btn-quiet" onClick={() => setEditing(false)}>
              Cancel
            </button>
            <button className="btn" onClick={() => void commit()}>
              Save
            </button>
          </div>
        </div>
      </li>
    );
  }

  const due_ = formatDue(task.dueDate);
  return (
    <li
      className={`task${task.completed ? " task-done" : ""}`}
      tabIndex={0}
      onKeyDown={(e) => {
        if (e.target !== e.currentTarget) return;
        if (e.key === " ") {
          e.preventDefault();
          void onChange({ ...task, completed: !task.completed });
        } else if (e.key === "Enter") {
          e.preventDefault();
          startEdit();
        } else if (e.key === "Backspace" && (e.metaKey || e.ctrlKey)) {
          e.preventDefault();
          e.stopPropagation();
          void onDelete(task);
        }
      }}
    >
      <input
        type="checkbox"
        checked={task.completed}
        onChange={() => void onChange({ ...task, completed: !task.completed })}
        aria-label={`Mark “${task.name}” ${task.completed ? "incomplete" : "complete"}`}
        tabIndex={-1}
      />
      <div className="task-main" onDoubleClick={startEdit}>
        <span className="task-name">{task.name}</span>
        {task.description && <span className="task-desc">{task.description}</span>}
      </div>
      <span className="task-meta">
        <span className={`prio prio-${task.priority}`} title={`${task.priority} priority`} aria-label={`${task.priority} priority`} />
        <span>{EFFORT_LABELS[task.effort]}</span>
        {due_ && <span className={due_.startsWith("overdue") ? "overdue" : ""}>{due_}</span>}
      </span>
      <span className="task-actions">
        <button className="btn btn-quiet" onClick={startEdit} aria-label={`Edit ${task.name}`}>
          Edit
        </button>
        <button className="btn btn-quiet btn-danger" onClick={() => void onDelete(task)} aria-label={`Delete ${task.name}`}>
          Delete
        </button>
      </span>
    </li>
  );
}

/** Lightweight task checklist: complete, edit, delete. */
export function TaskList({ tasks, onChange, onDelete }: Props) {
  if (!tasks.length) return null;
  return (
    <ul className="tasks">
      {tasks.map((t) => (
        <TaskRow key={t.id} task={t} onChange={onChange} onDelete={onDelete} />
      ))}
    </ul>
  );
}
