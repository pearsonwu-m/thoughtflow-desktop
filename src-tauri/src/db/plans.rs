//! Saved plans and lightweight tasks.

use super::json_vec;
use super::models::{
    normalize_date, normalize_effort, normalize_priority, normalize_status, Plan, PlanStep, Task,
};
use super::thoughts::ensure_changed;
use rusqlite::{params, Connection, OptionalExtension, Row};

const PLAN_COLUMNS: &str =
    "id, thought_id, title, objective, why, steps, next_action, obstacles, deadline, status, created_at, updated_at";

fn plan_from_row(row: &Row) -> rusqlite::Result<Plan> {
    let steps: Vec<PlanStep> = serde_json::from_str(&row.get::<_, String>(5)?).unwrap_or_default();
    Ok(Plan {
        id: row.get(0)?,
        thought_id: row.get(1)?,
        title: row.get(2)?,
        objective: row.get(3)?,
        why: row.get(4)?,
        steps,
        next_action: row.get(6)?,
        obstacles: json_vec(&row.get::<_, String>(7)?),
        deadline: row.get(8)?,
        status: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

/// Cleans user- or model-supplied plan fields before they are stored.
pub fn normalize_plan(mut p: Plan) -> Plan {
    p.title = p.title.trim().to_string();
    if p.title.is_empty() {
        p.title = "Untitled plan".into();
    }
    p.status = normalize_status(&p.status);
    p.deadline = normalize_date(p.deadline.as_deref());
    p.steps.retain(|s| !s.title.trim().is_empty());
    p.obstacles.retain(|o| !o.trim().is_empty());
    p
}

pub fn insert_plan(conn: &Connection, p: &Plan) -> rusqlite::Result<()> {
    conn.execute(
        &format!("INSERT INTO plans ({PLAN_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)"),
        params![
            p.id,
            p.thought_id,
            p.title,
            p.objective,
            p.why,
            serde_json::to_string(&p.steps).unwrap_or_else(|_| "[]".into()),
            p.next_action,
            serde_json::to_string(&p.obstacles).unwrap_or_else(|_| "[]".into()),
            p.deadline,
            p.status,
            p.created_at,
            p.updated_at
        ],
    )?;
    Ok(())
}

pub fn update_plan(conn: &Connection, p: &Plan) -> rusqlite::Result<()> {
    let n = conn.execute(
        "UPDATE plans SET title = ?2, objective = ?3, why = ?4, steps = ?5, next_action = ?6,
                          obstacles = ?7, deadline = ?8, status = ?9, updated_at = ?10
         WHERE id = ?1",
        params![
            p.id,
            p.title,
            p.objective,
            p.why,
            serde_json::to_string(&p.steps).unwrap_or_else(|_| "[]".into()),
            p.next_action,
            serde_json::to_string(&p.obstacles).unwrap_or_else(|_| "[]".into()),
            p.deadline,
            p.status,
            p.updated_at
        ],
    )?;
    ensure_changed(n)
}

pub fn get_plan(conn: &Connection, id: &str) -> rusqlite::Result<Option<Plan>> {
    conn.query_row(
        &format!("SELECT {PLAN_COLUMNS} FROM plans WHERE id = ?1"),
        [id],
        plan_from_row,
    )
    .optional()
}

pub fn list_plans(conn: &Connection) -> rusqlite::Result<Vec<Plan>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {PLAN_COLUMNS} FROM plans
         ORDER BY CASE status WHEN 'active' THEN 0 WHEN 'done' THEN 1 ELSE 2 END, updated_at DESC"
    ))?;
    let rows = stmt.query_map([], plan_from_row)?;
    rows.collect()
}

pub fn plans_for_thought(conn: &Connection, thought_id: &str) -> rusqlite::Result<Vec<Plan>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {PLAN_COLUMNS} FROM plans WHERE thought_id = ?1 ORDER BY created_at ASC"
    ))?;
    let rows = stmt.query_map([thought_id], plan_from_row)?;
    rows.collect()
}

pub fn delete_plan(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    Ok(conn.execute("DELETE FROM plans WHERE id = ?1", [id])? > 0)
}

// --- Tasks ------------------------------------------------------------------

const TASK_COLUMNS: &str = "id, thought_id, plan_id, name, description, priority, effort, due_date, completed, completed_at, created_at, updated_at";

/// Open tasks first, then by priority, due date, and age.
const TASK_ORDER: &str = "ORDER BY completed ASC,
    CASE priority WHEN 'high' THEN 0 WHEN 'medium' THEN 1 ELSE 2 END,
    CASE WHEN due_date IS NULL THEN 1 ELSE 0 END, due_date ASC, created_at ASC";

fn task_from_row(row: &Row) -> rusqlite::Result<Task> {
    Ok(Task {
        id: row.get(0)?,
        thought_id: row.get(1)?,
        plan_id: row.get(2)?,
        name: row.get(3)?,
        description: row.get(4)?,
        priority: row.get(5)?,
        effort: row.get(6)?,
        due_date: row.get(7)?,
        completed: row.get(8)?,
        completed_at: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

pub fn normalize_task(mut t: Task) -> Task {
    t.name = t.name.trim().to_string();
    t.description = t.description.trim().to_string();
    t.priority = normalize_priority(&t.priority);
    t.effort = normalize_effort(&t.effort);
    t.due_date = normalize_date(t.due_date.as_deref());
    t
}

pub fn insert_task(conn: &Connection, t: &Task) -> rusqlite::Result<()> {
    conn.execute(
        &format!("INSERT INTO tasks ({TASK_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)"),
        params![
            t.id,
            t.thought_id,
            t.plan_id,
            t.name,
            t.description,
            t.priority,
            t.effort,
            t.due_date,
            t.completed,
            t.completed_at,
            t.created_at,
            t.updated_at
        ],
    )?;
    Ok(())
}

pub fn update_task(conn: &Connection, t: &Task) -> rusqlite::Result<()> {
    let n = conn.execute(
        "UPDATE tasks SET name = ?2, description = ?3, priority = ?4, effort = ?5, due_date = ?6,
                          completed = ?7, completed_at = ?8, updated_at = ?9
         WHERE id = ?1",
        params![
            t.id,
            t.name,
            t.description,
            t.priority,
            t.effort,
            t.due_date,
            t.completed,
            t.completed_at,
            t.updated_at
        ],
    )?;
    ensure_changed(n)
}

pub fn get_task(conn: &Connection, id: &str) -> rusqlite::Result<Option<Task>> {
    conn.query_row(
        &format!("SELECT {TASK_COLUMNS} FROM tasks WHERE id = ?1"),
        [id],
        task_from_row,
    )
    .optional()
}

pub fn list_tasks(conn: &Connection) -> rusqlite::Result<Vec<Task>> {
    let mut stmt = conn.prepare(&format!("SELECT {TASK_COLUMNS} FROM tasks {TASK_ORDER}"))?;
    let rows = stmt.query_map([], task_from_row)?;
    rows.collect()
}

pub fn tasks_for_thought(conn: &Connection, thought_id: &str) -> rusqlite::Result<Vec<Task>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {TASK_COLUMNS} FROM tasks WHERE thought_id = ?1 {TASK_ORDER}"
    ))?;
    let rows = stmt.query_map([thought_id], task_from_row)?;
    rows.collect()
}

pub fn delete_task(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    Ok(conn.execute("DELETE FROM tasks WHERE id = ?1", [id])? > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::db::thoughts::fixtures;

    fn task(name: &str, priority: &str, completed: bool) -> Task {
        normalize_task(Task {
            id: crate::util::new_id(),
            thought_id: None,
            plan_id: None,
            name: name.into(),
            description: String::new(),
            priority: priority.into(),
            effort: "bogus".into(),
            due_date: Some("not a date".into()),
            completed,
            completed_at: None,
            created_at: 1,
            updated_at: 1,
        })
    }

    #[test]
    fn tasks_sort_open_and_important_first() {
        let conn = open_in_memory();
        for t in [
            task("done", "high", true),
            task("low", "low", false),
            task("high", "high", false),
        ] {
            insert_task(&conn, &t).unwrap();
        }
        let names: Vec<String> = list_tasks(&conn)
            .unwrap()
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert_eq!(names, vec!["high", "low", "done"]);
        let first = &list_tasks(&conn).unwrap()[0];
        assert_eq!(first.effort, "short");
        assert_eq!(first.due_date, None);
    }

    #[test]
    fn plans_round_trip_and_follow_their_thought() {
        let conn = open_in_memory();
        let thought = fixtures::thought(&conn, "Website", "work on my website");
        let plan = normalize_plan(Plan {
            id: crate::util::new_id(),
            thought_id: Some(thought.id.clone()),
            title: "  Ship the website ".into(),
            objective: "Publish v1".into(),
            why: "Portfolio".into(),
            steps: vec![
                PlanStep {
                    title: "Pick a template".into(),
                    detail: String::new(),
                    done: false,
                },
                PlanStep {
                    title: " ".into(),
                    detail: String::new(),
                    done: false,
                },
            ],
            next_action: "Open the repo".into(),
            obstacles: vec!["Time".into()],
            deadline: Some("2026-10-15".into()),
            status: "weird".into(),
            created_at: 1,
            updated_at: 1,
        });
        insert_plan(&conn, &plan).unwrap();
        let loaded = get_plan(&conn, &plan.id).unwrap().unwrap();
        assert_eq!(loaded.title, "Ship the website");
        assert_eq!(loaded.steps.len(), 1);
        assert_eq!(loaded.status, "active");
        assert_eq!(plans_for_thought(&conn, &thought.id).unwrap().len(), 1);

        crate::db::thoughts::delete(&conn, &thought.id).unwrap();
        assert!(
            list_plans(&conn).unwrap().is_empty(),
            "plans are forgotten with their thought"
        );
    }
}
