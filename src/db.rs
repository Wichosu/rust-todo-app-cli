use crate::todo::Priority;
use crate::todo::Todo;
use chrono::Utc;
use rusqlite::{Connection, Result};

pub fn connect() -> Result<Connection, rusqlite::Error> {
    let conn = Connection::open("todos.db")?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS todos (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      text TEXT NOT NULL,
      completed INTEGER NOT NULL DEFAULT 0,
      created_at TEXT NOT NULL,
      completed_at TEXT,
      priority TEXT NOT NULL DEFAULT 'medium',
      due_date TEXT
      )",
        [],
    )?;

    let _ = conn.execute("ALTER TABLE todos ADD COLUMN due_date TEXT", []);

    Ok(conn)
}

pub fn add_task(conn: &Connection, text: &str, priority: Priority, due_date: Option<&str>) -> Result<()> {
    let now = Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO todos (text, created_at, priority, due_date) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![text, now, priority.as_str(), due_date],
    )?;
    Ok(())
}

pub fn delete_task(conn: &Connection, id: &i64) -> Result<()> {
    conn.execute("DELETE FROM todos WHERE id = ?1", [id])?;
    Ok(())
}

pub fn delete_all_completed(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM todos WHERE completed = 1", [])?;
    Ok(())
}

pub fn mark_completed(conn: &Connection, id: &i64) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE todos SET completed=true, completed_at=?1 WHERE id=?2",
        [&now, &id.to_string()],
    )?;
    Ok(())
}

pub fn mark_incomplete(conn: &Connection, id: &i64) -> Result<()> {
    conn.execute(
        "UPDATE todos SET completed=false, completed_at=NULL WHERE id=?1",
        [id],
    )?;
    Ok(())
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
    let mut sql = String::from(
        "SELECT id, text, completed, created_at, completed_at, priority, due_date FROM todos",
    );
    let mut conditions = Vec::new();
    let mut params: Vec<rusqlite::types::Value> = Vec::new();

    if let Some(c) = completed {
        conditions.push(format!("completed = ?{}", conditions.len() + 1));
        params.push(rusqlite::types::Value::Integer(c as i64));
    }
    if let Some(p) = priority {
        conditions.push(format!("priority = ?{}", conditions.len() + 1));
        params.push(rusqlite::types::Value::Text(p.as_str().to_string()));
    }
    if let Some(d) = due_date {
        conditions.push(format!("due_date = ?{}", conditions.len() + 1));
        params.push(rusqlite::types::Value::Text(d.to_string()));
    }
    if let Some(before) = overdue_before {
        conditions.push(format!("due_date IS NOT NULL AND due_date < ?{}", conditions.len() + 1));
        params.push(rusqlite::types::Value::Text(before.to_string()));
    }
    if let Some(before) = due_before {
        conditions.push(format!("due_date IS NOT NULL AND due_date < ?{}", conditions.len() + 1));
        params.push(rusqlite::types::Value::Text(before.to_string()));
    }
    if let Some(after) = due_after {
        conditions.push(format!("due_date IS NOT NULL AND due_date > ?{}", conditions.len() + 1));
        params.push(rusqlite::types::Value::Text(after.to_string()));
    }

    if !conditions.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&conditions.join(" AND "));
    }

    let mut stmt = conn.prepare(&sql)?;

    let rows = stmt.query_map(rusqlite::params_from_iter(params), |row| {
        Ok(Todo {
            id: row.get(0)?,
            text: row.get(1)?,
            completed: row.get(2)?,
            created_at: row.get(3)?,
            completed_at: row.get(4)?,
            priority: row.get(5)?,
            due_date: row.get(6)?,
        })
    })?;

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
    let mut sql = String::from(
        "SELECT id, text, completed, created_at, completed_at, priority, due_date FROM todos WHERE text LIKE ?1",
    );
    let mut conditions = Vec::new();
    let mut params: Vec<rusqlite::types::Value> = Vec::new();

    params.push(rusqlite::types::Value::Text(format!("%{}%", query)));

    if let Some(c) = completed {
        conditions.push(format!("completed = ?{}", params.len() + 1));
        params.push(rusqlite::types::Value::Integer(c as i64));
    }
    if let Some(p) = priority {
        conditions.push(format!("priority = ?{}", params.len() + 1));
        params.push(rusqlite::types::Value::Text(p.as_str().to_string()));
    }
    if let Some(d) = due_date {
        conditions.push(format!("due_date = ?{}", params.len() + 1));
        params.push(rusqlite::types::Value::Text(d.to_string()));
    }
    if let Some(before) = overdue_before {
        conditions.push(format!("due_date IS NOT NULL AND due_date < ?{}", params.len() + 1));
        params.push(rusqlite::types::Value::Text(before.to_string()));
    }

    for condition in conditions {
        sql.push_str(&format!(" AND {}", condition));
    }

    let mut stmt = conn.prepare(&sql)?;

    let rows = stmt.query_map(rusqlite::params_from_iter(params), |row| {
        Ok(Todo {
            id: row.get(0)?,
            text: row.get(1)?,
            completed: row.get(2)?,
            created_at: row.get(3)?,
            completed_at: row.get(4)?,
            priority: row.get(5)?,
            due_date: row.get(6)?,
        })
    })?;

    let mut todos = Vec::new();
    for row in rows {
        todos.push(row?);
    }
    Ok(todos)
}
