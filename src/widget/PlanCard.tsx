import { useState } from "react";
import { formatDue } from "../lib/format";
import type { Plan, PlanStatus } from "../types";

interface Props {
  plan: Plan;
  onChange: (plan: Plan) => Promise<void>;
  onDelete?: (plan: Plan) => Promise<void>;
  compact?: boolean;
}

const STATUSES: PlanStatus[] = ["active", "done", "archived"];

function lines(text: string): string[] {
  return text
    .split("\n")
    .map((l) => l.replace(/^\s*(?:[-*•]|\d+[.)])\s*/, "").trim())
    .filter(Boolean);
}

/** A saved plan: objective, ordered steps, next action, obstacles, deadline, status. */
export function PlanCard({ plan, onChange, onDelete, compact }: Props) {
  const [editing, setEditing] = useState(false);
  const [form, setForm] = useState(() => toForm(plan));

  const startEdit = () => {
    setForm(toForm(plan));
    setEditing(true);
  };

  const commit = async () => {
    const steps = lines(form.steps).map((title, i) => {
      const existing = plan.steps.find((s) => s.title === title) ?? plan.steps[i];
      return { title, detail: existing?.title === title ? existing.detail : "", done: existing?.title === title ? existing.done : false };
    });
    await onChange({
      ...plan,
      title: form.title.trim() || plan.title,
      objective: form.objective.trim(),
      why: form.why.trim(),
      steps,
      nextAction: form.nextAction.trim(),
      obstacles: lines(form.obstacles),
      deadline: form.deadline || null,
    });
    setEditing(false);
  };

  const toggleStep = (index: number) =>
    void onChange({ ...plan, steps: plan.steps.map((s, i) => (i === index ? { ...s, done: !s.done } : s)) });

  if (editing) {
    const field = (key: keyof typeof form, label: string, multiline = false) => (
      <label className="plan-field">
        <span className="label">{label}</span>
        {multiline ? (
          <textarea value={form[key]} rows={4} onChange={(e) => setForm({ ...form, [key]: e.target.value })} />
        ) : (
          <input value={form[key]} onChange={(e) => setForm({ ...form, [key]: e.target.value })} />
        )}
      </label>
    );
    return (
      <article
        className="plan plan-editing"
        onKeyDown={(e) => {
          if (e.key === "Escape") {
            e.stopPropagation();
            setEditing(false);
          } else if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
            e.preventDefault();
            e.stopPropagation();
            void commit();
          }
        }}
      >
        {field("title", "Title")}
        {field("objective", "Objective")}
        {field("why", "Why it matters")}
        {field("steps", "Steps (one per line)", true)}
        {field("nextAction", "Next action")}
        {field("obstacles", "Possible obstacles (one per line)", true)}
        <label className="plan-field">
          <span className="label">Deadline</span>
          <input type="date" value={form.deadline} onChange={(e) => setForm({ ...form, deadline: e.target.value })} />
        </label>
        <div className="plan-actions">
          <span className="spacer" />
          <button className="btn btn-quiet" onClick={() => setEditing(false)}>
            Cancel
          </button>
          <button className="btn" onClick={() => void commit()}>
            Save plan
          </button>
        </div>
      </article>
    );
  }

  const due = formatDue(plan.deadline);
  return (
    <article className={`plan${compact ? " plan-compact" : ""}`} aria-label={`Plan: ${plan.title}`}>
      <header className="plan-head">
        <h3 className="plan-title">{plan.title}</h3>
        <select
          className="plan-status"
          value={plan.status}
          onChange={(e) => void onChange({ ...plan, status: e.target.value as PlanStatus })}
          aria-label="Plan status"
        >
          {STATUSES.map((s) => (
            <option key={s} value={s}>
              {s}
            </option>
          ))}
        </select>
      </header>
      {plan.objective && <p className="plan-objective">{plan.objective}</p>}
      {plan.why && !compact && <p className="plan-why">{plan.why}</p>}
      {plan.steps.length > 0 && (
        <ol className="plan-steps">
          {plan.steps.map((step, i) => (
            <li key={i} className={step.done ? "done" : ""}>
              <label>
                <input type="checkbox" checked={step.done} onChange={() => toggleStep(i)} />
                <span>
                  {step.title}
                  {step.detail && !compact && <span className="step-detail"> — {step.detail}</span>}
                </span>
              </label>
            </li>
          ))}
        </ol>
      )}
      <dl className="plan-facts">
        {plan.nextAction && (
          <div>
            <dt className="label">Next action</dt>
            <dd>{plan.nextAction}</dd>
          </div>
        )}
        {plan.obstacles.length > 0 && !compact && (
          <div>
            <dt className="label">Obstacles</dt>
            <dd>
              <ul>
                {plan.obstacles.map((o, i) => (
                  <li key={i}>{o}</li>
                ))}
              </ul>
            </dd>
          </div>
        )}
        {due && (
          <div>
            <dt className="label">Deadline</dt>
            <dd className={due.startsWith("overdue") ? "overdue" : ""}>{due}</dd>
          </div>
        )}
      </dl>
      <footer className="plan-actions">
        <button className="btn btn-quiet" onClick={startEdit}>
          Edit
        </button>
        {onDelete && (
          <button className="btn btn-quiet btn-danger" onClick={() => void onDelete(plan)}>
            Delete
          </button>
        )}
      </footer>
    </article>
  );
}

function toForm(plan: Plan) {
  return {
    title: plan.title,
    objective: plan.objective,
    why: plan.why,
    steps: plan.steps.map((s) => s.title).join("\n"),
    nextAction: plan.nextAction,
    obstacles: plan.obstacles.join("\n"),
    deadline: plan.deadline ?? "",
  };
}
