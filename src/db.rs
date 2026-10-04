use crate::filter::{self, DateFilter, TodoFilter};
use crate::todo::{sort_todos, Priority, Repeat, Todo};
use chrono::{Days, Months, NaiveDate, Utc};
use rusqlite::{Connection, OptionalExtension, Result};

const SELECT_TODOS: &str =
    "SELECT id, text, completed, created_at, completed_at, priority, due_date, repeat FROM todos";

struct Query {
    conditions: Vec<String>,
    params: Vec<rusqlite::types::Value>,
}

impl Query {
    fn new() -> Query {
        Query {
            conditions: Vec::new(),
            params: Vec::new(),
        }
    }

    fn push(&mut self, condition: &str, param: rusqlite::types::Value) {
        let n = self.params.len() + 1;
        self.conditions.push(format!("{} ?{}", condition, n));
        self.params.push(param);
    }

    fn sql_with_where(&self, base: &str) -> String {
        let mut sql = base.to_string();
        if !self.conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&self.conditions.join(" AND "));
        }
        sql
    }
}

fn map_row(row: &rusqlite::Row<'_>) -> Result<Todo> {
    Ok(Todo {
        id: row.get(0)?,
        text: row.get(1)?,
        completed: row.get(2)?,
        created_at: row.get(3)?,
        completed_at: row.get(4)?,
        priority: row.get(5)?,
        due_date: row.get(6)?,
        repeat: row.get(7)?,
    })
}

pub fn connect() -> Result<Connection, rusqlite::Error> {
    let conn = Connection::open("todos.db")?;

    init_schema(&conn)?;

    let _ = conn.execute("ALTER TABLE todos ADD COLUMN due_date TEXT", []);
    let _ = conn.execute(
        "ALTER TABLE todos ADD COLUMN repeat TEXT NOT NULL DEFAULT 'none'",
        [],
    );
    let _ = conn.execute("ALTER TABLE undo_log ADD COLUMN before_repeat TEXT", []);

    Ok(conn)
}

pub fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS todos (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      text TEXT NOT NULL,
      completed INTEGER NOT NULL DEFAULT 0,
      created_at TEXT NOT NULL,
      completed_at TEXT,
      priority TEXT NOT NULL DEFAULT 'medium',
      due_date TEXT,
      repeat TEXT NOT NULL DEFAULT 'none'
      )",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS undo_log (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      batch INTEGER NOT NULL,
      op TEXT NOT NULL,
      todo_id INTEGER NOT NULL,
      before_text TEXT,
      before_completed INTEGER,
      before_completed_at TEXT,
      before_created_at TEXT,
      before_priority TEXT,
      before_due_date TEXT,
      before_repeat TEXT
      )",
        [],
    )?;

    Ok(())
}

pub fn add_task(
    conn: &Connection,
    text: &str,
    priority: Priority,
    due_date: Option<&str>,
    repeat: Repeat,
    batch: i64,
) -> Result<()> {
    let now = Utc::now().to_rfc3339();

    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO todos (text, created_at, priority, due_date, repeat) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![text, now, priority.as_str(), due_date, repeat.as_str()],
    )?;
    let id = tx.last_insert_rowid();
    crate::undo::record(&tx, batch, crate::undo::UndoOp::Add, id, None)?;
    tx.commit()?;
    Ok(())
}

pub fn delete_task(conn: &Connection, id: &i64, batch: i64) -> Result<()> {
    let tx = conn.unchecked_transaction()?;

    let before: Option<Todo> = tx
        .query_row(
            &format!("{SELECT_TODOS} WHERE id = ?1"),
            [id],
            map_row,
        )
        .optional()?;

    tx.execute("DELETE FROM todos WHERE id = ?1", [id])?;

    if let Some(before) = &before {
        crate::undo::record(&tx, batch, crate::undo::UndoOp::Delete, *id, Some(before))?;
    }

    tx.commit()?;
    Ok(())
}

pub fn delete_all_completed(conn: &Connection, batch: i64) -> Result<()> {
    let tx = conn.unchecked_transaction()?;

    let completed: Vec<Todo> = {
        let mut stmt = tx.prepare(&format!("{SELECT_TODOS} WHERE completed = 1"))?;
        let rows = stmt.query_map([], map_row)?;
        rows.collect::<Result<Vec<_>>>()?
    };

    tx.execute("DELETE FROM todos WHERE completed = 1", [])?;

    for todo in &completed {
        crate::undo::record(&tx, batch, crate::undo::UndoOp::Delete, todo.id, Some(todo))?;
    }

    tx.commit()?;
    Ok(())
}

fn set_completed(conn: &Connection, id: &i64, completed: bool, batch: i64) -> Result<()> {
    let tx = conn.unchecked_transaction()?;

    let before: Option<Todo> = tx
        .query_row(
            &format!("{SELECT_TODOS} WHERE id = ?1"),
            [id],
            map_row,
        )
        .optional()?;

    if completed {
        let now = Utc::now().to_rfc3339();
        tx.execute(
            "UPDATE todos SET completed=true, completed_at=?1 WHERE id=?2",
            [&now, &id.to_string()],
        )?;
    } else {
        tx.execute(
            "UPDATE todos SET completed=false, completed_at=NULL WHERE id=?1",
            [id],
        )?;
    }

    if let Some(before) = &before {
        let op = if completed {
            crate::undo::UndoOp::Done
        } else {
            crate::undo::UndoOp::Undone
        };
        crate::undo::record(&tx, batch, op, *id, Some(before))?;

        if completed
            && !before.completed
            && before.repeat != Repeat::None
            && let Some(next_due) = next_occurrence(
                before.repeat,
                before.due_date.as_deref(),
                &filter::today(),
            )
        {
            tx.execute(
                "INSERT INTO todos (text, created_at, priority, due_date, repeat) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    before.text,
                    Utc::now().to_rfc3339(),
                    before.priority.as_str(),
                    next_due,
                    before.repeat.as_str()
                ],
            )?;
            let copy_id = tx.last_insert_rowid();
            crate::undo::record(&tx, batch, crate::undo::UndoOp::Add, copy_id, None)?;
        }
    }

    tx.commit()?;
    Ok(())
}

pub fn mark_completed(conn: &Connection, id: &i64, batch: i64) -> Result<()> {
    set_completed(conn, id, true, batch)
}

pub fn mark_incomplete(conn: &Connection, id: &i64, batch: i64) -> Result<()> {
    set_completed(conn, id, false, batch)
}

fn add_period(date: NaiveDate, repeat: Repeat) -> Option<NaiveDate> {
    match repeat {
        Repeat::Daily => date.checked_add_days(Days::new(1)),
        Repeat::Weekly => date.checked_add_days(Days::new(7)),
        Repeat::Monthly => date.checked_add_months(Months::new(1)),
        Repeat::None => None,
    }
}

pub(crate) fn next_occurrence(
    repeat: Repeat,
    due_date: Option<&str>,
    today: &str,
) -> Option<String> {
    if repeat == Repeat::None {
        return None;
    }
    let today_date = NaiveDate::parse_from_str(today, "%Y-%m-%d").ok()?;
    let anchor = due_date
        .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        .unwrap_or(today_date);

    let mut next = add_period(anchor, repeat)?;
    while next <= today_date {
        next = add_period(next, repeat)?;
    }
    Some(next.format("%Y-%m-%d").to_string())
}

pub fn update_task_text(
    conn: &Connection,
    id: &i64,
    new_text: &str,
    batch: i64,
) -> Result<()> {
    let tx = conn.unchecked_transaction()?;

    let before: Option<Todo> = tx
        .query_row(
            &format!("{SELECT_TODOS} WHERE id = ?1"),
            [id],
            map_row,
        )
        .optional()?;

    tx.execute(
        "UPDATE todos SET text = ?1 WHERE id = ?2",
        [new_text, &id.to_string()],
    )?;

    if let Some(before) = &before {
        crate::undo::record(&tx, batch, crate::undo::UndoOp::Edit, *id, Some(before))?;
    }

    tx.commit()?;
    Ok(())
}

fn insert_imported(tx: &rusqlite::Transaction, todo: &Todo) -> Result<i64> {
    tx.execute(
        "INSERT INTO todos (id, text, completed, created_at, completed_at, priority, due_date, repeat)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![
            todo.id,
            todo.text,
            todo.completed as i64,
            todo.created_at,
            todo.completed_at,
            todo.priority.as_str(),
            todo.due_date,
            todo.repeat.as_str()
        ],
    )?;
    Ok(tx.last_insert_rowid())
}

pub fn import_tasks(conn: &Connection, todos: &[Todo], batch: i64) -> Result<usize> {
    let tx = conn.unchecked_transaction()?;

    let existing: std::collections::HashSet<i64> = {
        let mut stmt = tx.prepare("SELECT id FROM todos")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        rows.collect::<Result<_>>()?
    };

    let mut inserted = 0;
    for todo in todos {
        let new_id = if existing.contains(&todo.id) {
            tx.execute(
                "INSERT INTO todos (text, completed, created_at, completed_at, priority, due_date, repeat)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    todo.text,
                    todo.completed as i64,
                    todo.created_at,
                    todo.completed_at,
                    todo.priority.as_str(),
                    todo.due_date,
                    todo.repeat.as_str()
                ],
            )?;
            tx.last_insert_rowid()
        } else {
            insert_imported(&tx, todo)?
        };
        crate::undo::record(&tx, batch, crate::undo::UndoOp::Add, new_id, None)?;
        inserted += 1;
    }

    tx.commit()?;
    Ok(inserted)
}

pub fn replace_all_tasks(
    conn: &Connection,
    todos: &[Todo],
    batch: i64,
) -> Result<(usize, usize)> {
    let tx = conn.unchecked_transaction()?;

    let existing: Vec<Todo> = {
        let mut stmt = tx.prepare(SELECT_TODOS)?;
        let rows = stmt.query_map([], map_row)?;
        rows.collect::<Result<Vec<_>>>()?
    };

    tx.execute("DELETE FROM todos", [])?;
    for todo in &existing {
        crate::undo::record(&tx, batch, crate::undo::UndoOp::Delete, todo.id, Some(todo))?;
    }

    for todo in todos {
        insert_imported(&tx, todo)?;
        crate::undo::record(&tx, batch, crate::undo::UndoOp::Add, todo.id, None)?;
    }

    tx.commit()?;
    Ok((existing.len(), todos.len()))
}

pub fn list_tasks(
    conn: &Connection,
    completed: Option<bool>,
    priority: Option<Priority>,
    due_date: Option<&str>,
    overdue_before: Option<&str>,
    due_before: Option<&str>,
    due_after: Option<&str>,
) -> Result<Vec<Todo>> {
    let mut query = Query::new();
    if let Some(c) = completed {
        query.push("completed =", rusqlite::types::Value::Integer(c as i64));
    }
    if let Some(p) = priority {
        query.push("priority =", rusqlite::types::Value::Text(p.as_str().to_string()));
    }
    if let Some(d) = due_date {
        query.push("due_date =", rusqlite::types::Value::Text(d.to_string()));
    }
    if let Some(before) = overdue_before {
        query.push(
            "due_date IS NOT NULL AND due_date <",
            rusqlite::types::Value::Text(before.to_string()),
        );
    }
    if let Some(before) = due_before {
        query.push(
            "due_date IS NOT NULL AND due_date <",
            rusqlite::types::Value::Text(before.to_string()),
        );
    }
    if let Some(after) = due_after {
        query.push(
            "due_date IS NOT NULL AND due_date >",
            rusqlite::types::Value::Text(after.to_string()),
        );
    }

    let sql = query.sql_with_where(SELECT_TODOS);
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(query.params), map_row)?;

    let mut todos = Vec::new();
    for row in rows {
        todos.push(row?);
    }
    Ok(todos)
}

pub fn search_tasks(
    conn: &Connection,
    query: &str,
    completed: Option<bool>,
    priority: Option<Priority>,
    due_date: Option<&str>,
    overdue_before: Option<&str>,
) -> Result<Vec<Todo>> {
    let mut query_builder = Query::new();
    query_builder.push(
        "text LIKE",
        rusqlite::types::Value::Text(format!("%{}%", query)),
    );
    if let Some(c) = completed {
        query_builder.push("completed =", rusqlite::types::Value::Integer(c as i64));
    }
    if let Some(p) = priority {
        query_builder.push("priority =", rusqlite::types::Value::Text(p.as_str().to_string()));
    }
    if let Some(d) = due_date {
        query_builder.push("due_date =", rusqlite::types::Value::Text(d.to_string()));
    }
    if let Some(before) = overdue_before {
        query_builder.push(
            "due_date IS NOT NULL AND due_date <",
            rusqlite::types::Value::Text(before.to_string()),
        );
    }

    let sql = query_builder.sql_with_where(SELECT_TODOS);
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(query_builder.params), map_row)?;

    let mut todos = Vec::new();
    for row in rows {
        todos.push(row?);
    }
    Ok(todos)
}

pub fn load_todos(conn: &Connection, filter: &TodoFilter) -> Result<Vec<Todo>> {
    let today = filter::today();
    let (due_date, overdue_before, due_before, due_after) = match filter.date {
        DateFilter::All => (None, None, None, None),
        DateFilter::DueToday => (Some(today.as_str()), None, None, None),
        DateFilter::Overdue => (None, Some(today.as_str()), None, None),
        DateFilter::Before => (None, None, Some(filter.date_value.as_str()), None),
        DateFilter::After => (None, None, None, Some(filter.date_value.as_str())),
    };
    let mut todos = list_tasks(
        conn,
        filter.completed(),
        filter.priority,
        due_date,
        overdue_before,
        due_before,
        due_after,
    )?;
    sort_todos(&mut todos);
    Ok(todos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_occurrence_daily_on_time() {
        assert_eq!(
            next_occurrence(Repeat::Daily, Some("2026-10-04"), "2026-10-04"),
            Some("2026-10-05".to_string())
        );
    }

    #[test]
    fn next_occurrence_daily_overdue_rolls_past_today() {
        assert_eq!(
            next_occurrence(Repeat::Daily, Some("2026-10-01"), "2026-10-04"),
            Some("2026-10-05".to_string())
        );
    }

    #[test]
    fn next_occurrence_future_anchor_keeps_schedule() {
        assert_eq!(
            next_occurrence(Repeat::Daily, Some("2026-10-10"), "2026-10-04"),
            Some("2026-10-11".to_string())
        );
    }

    #[test]
    fn next_occurrence_weekly_adds_seven_days() {
        assert_eq!(
            next_occurrence(Repeat::Weekly, Some("2026-10-05"), "2026-10-04"),
            Some("2026-10-12".to_string())
        );
    }

    #[test]
    fn next_occurrence_monthly_clamps_month_end() {
        assert_eq!(
            next_occurrence(Repeat::Monthly, Some("2026-01-31"), "2026-01-31"),
            Some("2026-02-28".to_string())
        );
    }

    #[test]
    fn next_occurrence_without_due_anchors_at_today() {
        assert_eq!(
            next_occurrence(Repeat::Daily, None, "2026-10-04"),
            Some("2026-10-05".to_string())
        );
    }

    #[test]
    fn next_occurrence_repeat_none_returns_none() {
        assert_eq!(
            next_occurrence(Repeat::None, Some("2026-10-04"), "2026-10-04"),
            None
        );
    }

    fn import_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        conn
    }

    fn row(id: i64, text: &str) -> Todo {
        Todo {
            id,
            text: text.to_string(),
            completed: false,
            created_at: "2026-01-01T00:00:00+00:00".to_string(),
            completed_at: None,
            priority: Priority::Medium,
            due_date: None,
            repeat: Repeat::None,
        }
    }

    fn journal_count(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM undo_log", [], |r| r.get(0))
            .unwrap()
    }

    fn ids(conn: &Connection) -> Vec<i64> {
        let mut stmt = conn.prepare("SELECT id FROM todos ORDER BY id").unwrap();
        let rows = stmt.query_map([], |r| r.get(0)).unwrap();
        rows.collect::<Result<Vec<_>>>().unwrap()
    }

    #[test]
    fn import_into_empty_preserves_ids() {
        let conn = import_conn();
        let todos = vec![row(1, "a"), row(2, "b")];

        let n = import_tasks(&conn, &todos, 1).unwrap();

        assert_eq!(n, 2);
        assert_eq!(ids(&conn), vec![1, 2]);
        assert_eq!(journal_count(&conn), 2);
    }

    #[test]
    fn import_collision_keeps_existing_and_reassigns() {
        let conn = import_conn();
        let batch = crate::undo::next_batch(&conn).unwrap();
        add_task(&conn, "original", Priority::High, Some("2026-10-05"), Repeat::Daily, batch)
            .unwrap();

        let n = import_tasks(&conn, &[row(1, "imported")], 2).unwrap();

        assert_eq!(n, 1);
        assert_eq!(ids(&conn), vec![1, 2]);
        let (id1_text, id1_priority): (String, String) = conn
            .query_row("SELECT text, priority FROM todos WHERE id = 1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(id1_text, "original");
        assert_eq!(id1_priority, "high");
        let id2_text: String = conn
            .query_row("SELECT text FROM todos WHERE id = 2", [], |r| r.get(0))
            .unwrap();
        assert_eq!(id2_text, "imported");
    }

    #[test]
    fn undo_import_removes_exactly_imported() {
        let conn = import_conn();
        let batch = crate::undo::next_batch(&conn).unwrap();
        add_task(&conn, "original", Priority::Medium, None, Repeat::None, batch).unwrap();

        let batch = crate::undo::next_batch(&conn).unwrap();
        let todos = vec![row(1, "clash"), row(9, "fresh")];
        import_tasks(&conn, &todos, batch).unwrap();
        assert_eq!(ids(&conn), vec![1, 2, 9]);

        crate::undo::undo_last(&conn).unwrap().expect("journal pops");

        assert_eq!(ids(&conn), vec![1]);
        assert_eq!(journal_count(&conn), 1);
        let text: String = conn
            .query_row("SELECT text FROM todos", [], |r| r.get(0))
            .unwrap();
        assert_eq!(text, "original");
    }

    #[test]
    fn replace_journals_delete_and_add_in_one_batch_and_undo_restores() {
        let conn = import_conn();
        let batch = crate::undo::next_batch(&conn).unwrap();
        add_task(&conn, "keep", Priority::High, Some("2026-10-05"), Repeat::Weekly, batch).unwrap();
        let old_id: i64 = conn.query_row("SELECT id FROM todos", [], |r| r.get(0)).unwrap();
        let created_at: String = conn
            .query_row("SELECT created_at FROM todos", [], |r| r.get(0))
            .unwrap();

        let batch = crate::undo::next_batch(&conn).unwrap();
        let (replaced, imported) =
            replace_all_tasks(&conn, &[row(99, "new")], batch).unwrap();

        assert_eq!((replaced, imported), (1, 1));
        assert_eq!(ids(&conn), vec![99]);
        let entries: i64 = conn
            .query_row("SELECT COUNT(*) FROM undo_log WHERE batch = ?1", [batch], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(entries, 2);

        crate::undo::undo_last(&conn).unwrap().expect("journal pops");

        assert_eq!(ids(&conn), vec![old_id]);
        let (text, priority, due, repeat, created): (String, String, Option<String>, String, String) =
            conn.query_row(
                "SELECT text, priority, due_date, repeat, created_at FROM todos",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert_eq!(text, "keep");
        assert_eq!(priority, "high");
        assert_eq!(due.as_deref(), Some("2026-10-05"));
        assert_eq!(repeat, "weekly");
        assert_eq!(created, created_at);
    }

    #[test]
    fn replace_into_empty_then_undo_reports_removed() {
        let conn = import_conn();
        let batch = crate::undo::next_batch(&conn).unwrap();
        replace_all_tasks(&conn, &[row(1, "a"), row(2, "b")], batch).unwrap();
        assert_eq!(ids(&conn), vec![1, 2]);

        let outcome = crate::undo::undo_last(&conn).unwrap().expect("journal pops");

        assert_eq!(ids(&conn), Vec::<i64>::new());
        assert_eq!(outcome.message(), "Reverted: 2 tasks removed.");
        assert_eq!(journal_count(&conn), 0);
    }

    #[test]
    fn import_empty_is_noop() {
        let conn = import_conn();
        let batch = crate::undo::next_batch(&conn).unwrap();
        add_task(&conn, "original", Priority::Medium, None, Repeat::None, batch).unwrap();

        let n = import_tasks(&conn, &[], 99).unwrap();

        assert_eq!(n, 0);
        assert_eq!(ids(&conn), vec![1]);
        assert_eq!(journal_count(&conn), 1);
    }
}
