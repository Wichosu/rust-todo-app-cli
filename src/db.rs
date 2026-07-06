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
      priority TEXT NOT NULL DEFAULT 'medium'
      )",
        [],
    )?;

    Ok(conn)
}

pub fn add_task(conn: &Connection, text: &str, priority: Priority) -> Result<()> {
    let now = Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO todos (text, created_at, priority) VALUES (?1, ?2, ?3)",
        [text, &now, priority.as_str()],
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

pub fn list_tasks(conn: &Connection) -> Result<Vec<Todo>> {
    let mut stmt =
        conn.prepare("SELECT id, text, completed, created_at, completed_at, priority FROM todos")?;

    let rows = stmt.query_map([], |row| {
        Ok(Todo {
            id: row.get(0)?,
            text: row.get(1)?,
            completed: row.get(2)?,
            created_at: row.get(3)?,
            completed_at: row.get(4)?,
            priority: row.get(5)?,
        })
    })?;

    let mut todos = Vec::new();

    for row in rows {
        todos.push(row?);
    }

    Ok(todos)
}
