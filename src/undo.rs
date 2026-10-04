use rusqlite::{Connection, OptionalExtension, Result};

use crate::todo::Todo;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UndoOp {
    Add,
    Done,
    Undone,
    Edit,
    Delete,
}

impl UndoOp {
    fn as_str(&self) -> &'static str {
        match self {
            UndoOp::Add => "add",
            UndoOp::Done => "done",
            UndoOp::Undone => "undone",
            UndoOp::Edit => "edit",
            UndoOp::Delete => "delete",
        }
    }

    fn parse(s: &str) -> Result<UndoOp> {
        match s {
            "add" => Ok(UndoOp::Add),
            "done" => Ok(UndoOp::Done),
            "undone" => Ok(UndoOp::Undone),
            "edit" => Ok(UndoOp::Edit),
            "delete" => Ok(UndoOp::Delete),
            other => Err(rusqlite::Error::InvalidParameterName(format!(
                "unknown undo op: {other}"
            ))),
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct UndoEntry {
    pub op: UndoOp,
    pub text: Option<String>,
    pub completed: Option<bool>,
}

pub struct UndoOutcome {
    pub op: UndoOp,
    pub entries: Vec<UndoEntry>,
}

fn gone_message(total: usize) -> String {
    if total == 1 {
        "Undone: task was already removed.".to_string()
    } else {
        format!("Undone: {total} tasks were already removed.")
    }
}

fn updated_message(existing: usize, gone: usize) -> String {
    if gone == 0 {
        format!("Reverted: {existing} tasks updated.")
    } else {
        format!("Reverted: {existing} tasks updated, {gone} already removed.")
    }
}

impl UndoOutcome {
    pub fn message(&self) -> String {
        let affected: Vec<&UndoEntry> = self.entries.iter().filter(|e| e.op == self.op).collect();
        let total = affected.len();
        let existing: Vec<&UndoEntry> = affected
            .iter()
            .copied()
            .filter(|e| e.text.is_some())
            .collect();
        let gone = total - existing.len();

        match self.op {
            UndoOp::Add => {
                if existing.is_empty() {
                    gone_message(total)
                } else if existing.len() == 1 && gone == 0 {
                    format!(
                        "Reverted: task \"{}\" removed.",
                        existing[0].text.as_deref().unwrap_or_default()
                    )
                } else if gone == 0 {
                    format!("Reverted: {} tasks removed.", existing.len())
                } else {
                    format!(
                        "Reverted: {} tasks removed, {} already removed.",
                        existing.len(),
                        gone
                    )
                }
            }
            UndoOp::Done | UndoOp::Undone => {
                if existing.is_empty() {
                    gone_message(total)
                } else if total == 1 {
                    let state = if existing[0].completed.unwrap_or(false) {
                        "completed"
                    } else {
                        "pending"
                    };
                    format!(
                        "Reverted: task \"{}\" is now {state}.",
                        existing[0].text.as_deref().unwrap_or_default()
                    )
                } else {
                    updated_message(existing.len(), gone)
                }
            }
            UndoOp::Edit => {
                if existing.is_empty() {
                    gone_message(total)
                } else if total == 1 {
                    format!(
                        "Reverted: task text is now \"{}\".",
                        existing[0].text.as_deref().unwrap_or_default()
                    )
                } else {
                    updated_message(existing.len(), gone)
                }
            }
            UndoOp::Delete => {
                if existing.is_empty() {
                    gone_message(total)
                } else if total == 1 {
                    format!(
                        "Reverted: task \"{}\" restored.",
                        existing[0].text.as_deref().unwrap_or_default()
                    )
                } else {
                    format!("Reverted: {} tasks restored.", existing.len())
                }
            }
        }
    }
}

pub fn next_batch(conn: &Connection) -> Result<i64> {
    conn.query_row("SELECT COALESCE(MAX(batch), 0) + 1 FROM undo_log", [], |r| {
        r.get(0)
    })
}

pub fn record(
    conn: &Connection,
    batch: i64,
    op: UndoOp,
    todo_id: i64,
    before: Option<&Todo>,
) -> Result<()> {
    let (text, completed, completed_at, created_at, priority, due_date, repeat) = match before {
        Some(t) => (
            Some(t.text.clone()),
            Some(t.completed as i64),
            t.completed_at.clone(),
            Some(t.created_at.clone()),
            Some(t.priority.as_str().to_string()),
            t.due_date.clone(),
            Some(t.repeat.as_str().to_string()),
        ),
        None => (None, None, None, None, None, None, None),
    };
    conn.execute(
        "INSERT INTO undo_log (batch, op, todo_id, before_text, before_completed, before_completed_at, before_created_at, before_priority, before_due_date, before_repeat)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        rusqlite::params![batch, op.as_str(), todo_id, text, completed, completed_at, created_at, priority, due_date, repeat],
    )?;
    Ok(())
}

struct JournalEntry {
    op: String,
    todo_id: i64,
    before_completed: Option<i64>,
    before_completed_at: Option<String>,
    before_text: Option<String>,
    before_created_at: Option<String>,
    before_priority: Option<String>,
    before_due_date: Option<String>,
    before_repeat: Option<String>,
}

pub fn undo_last(conn: &Connection) -> Result<Option<UndoOutcome>> {
    let batch: i64 = conn.query_row(
        "SELECT COALESCE(MAX(batch), 0) FROM undo_log",
        [],
        |r| r.get(0),
    )?;
    if batch == 0 {
        return Ok(None);
    }

    let entries: Vec<JournalEntry> = {
        let mut stmt = conn.prepare(
            "SELECT op, todo_id, before_completed, before_completed_at, before_text,
                    before_created_at, before_priority, before_due_date, before_repeat
             FROM undo_log WHERE batch = ?1 ORDER BY id DESC",
        )?;
        let rows = stmt.query_map([batch], |r| {
            Ok(JournalEntry {
                op: r.get(0)?,
                todo_id: r.get(1)?,
                before_completed: r.get(2)?,
                before_completed_at: r.get(3)?,
                before_text: r.get(4)?,
                before_created_at: r.get(5)?,
                before_priority: r.get(6)?,
                before_due_date: r.get(7)?,
                before_repeat: r.get(8)?,
            })
        })?;
        rows.collect::<Result<Vec<_>>>()?
    };

    let tx = conn.unchecked_transaction()?;
    let mut undone: Vec<UndoEntry> = Vec::new();
    let op = entries
        .last()
        .map(|e| UndoOp::parse(&e.op))
        .transpose()?;

    for entry in &entries {
        let entry_op = UndoOp::parse(&entry.op)?;
        let text: Option<String> = tx
            .query_row("SELECT text FROM todos WHERE id = ?1", [entry.todo_id], |r| {
                r.get(0)
            })
            .optional()?;

        match entry_op {
            UndoOp::Add => {
                tx.execute("DELETE FROM todos WHERE id = ?1", [entry.todo_id])?;
                undone.push(UndoEntry {
                    op: entry_op,
                    text,
                    completed: None,
                });
            }
            UndoOp::Done | UndoOp::Undone => {
                if text.is_some() {
                    tx.execute(
                        "UPDATE todos SET completed = ?1, completed_at = ?2 WHERE id = ?3",
                        rusqlite::params![
                            entry.before_completed,
                            entry.before_completed_at,
                            entry.todo_id
                        ],
                    )?;
                }
                undone.push(UndoEntry {
                    op: entry_op,
                    text,
                    completed: entry.before_completed.map(|c| c != 0),
                });
            }
            UndoOp::Edit => match (&text, &entry.before_text) {
                (Some(_), Some(restored)) => {
                    tx.execute(
                        "UPDATE todos SET text = ?1 WHERE id = ?2",
                        rusqlite::params![restored, entry.todo_id],
                    )?;
                    undone.push(UndoEntry {
                        op: entry_op,
                        text: Some(restored.clone()),
                        completed: None,
                    });
                }
                (Some(live), None) => undone.push(UndoEntry {
                    op: entry_op,
                    text: Some(live.clone()),
                    completed: None,
                }),
                (None, _) => undone.push(UndoEntry {
                    op: entry_op,
                    text: None,
                    completed: None,
                }),
            },
            UndoOp::Delete => {
                if text.is_none()
                    && let (Some(restored_text), Some(created_at)) =
                        (&entry.before_text, &entry.before_created_at)
                {
                    tx.execute(
                        "INSERT INTO todos (id, text, completed, created_at, completed_at, priority, due_date, repeat)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                        rusqlite::params![
                            entry.todo_id,
                            restored_text,
                            entry.before_completed.unwrap_or(0),
                            created_at,
                            entry.before_completed_at,
                            entry.before_priority
                                .clone()
                                .unwrap_or_else(|| "medium".to_string()),
                            entry.before_due_date,
                            entry.before_repeat
                                .clone()
                                .unwrap_or_else(|| "none".to_string())
                        ],
                    )?;
                }
                undone.push(UndoEntry {
                    op: entry_op,
                    text: entry.before_text.clone(),
                    completed: None,
                });
            }
        }
    }

    tx.execute("DELETE FROM undo_log WHERE batch = ?1", [batch])?;
    tx.commit()?;

    Ok(Some(UndoOutcome {
        op: op.expect("batch always has at least one entry"),
        entries: undone,
    }))
}

pub fn undo_n(conn: &Connection, count: usize) -> Result<Vec<UndoOutcome>> {
    let mut outcomes = Vec::new();
    for _ in 0..count {
        match undo_last(conn)? {
            Some(outcome) => outcomes.push(outcome),
            None => break,
        }
    }
    Ok(outcomes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::todo::{Priority, Repeat};

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        db::init_schema(&conn).unwrap();
        conn
    }

    fn task_count(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM todos", [], |r| r.get(0)).unwrap()
    }

    fn journal_count(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM undo_log", [], |r| r.get(0)).unwrap()
    }

    fn add(conn: &Connection, text: &str) {
        let batch = next_batch(conn).unwrap();
        db::add_task(conn, text, Priority::Medium, None, Repeat::None, batch).unwrap();
    }

    fn only_id(conn: &Connection) -> i64 {
        conn.query_row("SELECT id FROM todos", [], |r| r.get(0)).unwrap()
    }

    fn all_ids(conn: &Connection) -> Vec<i64> {
        let mut stmt = conn.prepare("SELECT id FROM todos ORDER BY id").unwrap();
        let rows = stmt.query_map([], |r| r.get(0)).unwrap();
        rows.collect::<Result<Vec<_>>>().unwrap()
    }

    fn completion(conn: &Connection, id: i64) -> (bool, Option<String>) {
        conn.query_row(
            "SELECT completed, completed_at FROM todos WHERE id = ?1",
            [id],
            |r| {
                Ok((
                    r.get::<_, i64>(0)? != 0,
                    r.get::<_, Option<String>>(1)?,
                ))
            },
        )
        .unwrap()
    }

    fn text_of(conn: &Connection, id: i64) -> String {
        conn.query_row("SELECT text FROM todos WHERE id = ?1", [id], |r| r.get(0))
            .unwrap()
    }

    fn edit(conn: &Connection, id: i64, new_text: &str) {
        let batch = next_batch(conn).unwrap();
        db::update_task_text(conn, &id, new_text, batch).unwrap();
    }

    fn delete(conn: &Connection, id: i64) {
        let batch = next_batch(conn).unwrap();
        db::delete_task(conn, &id, batch).unwrap();
    }

    fn row_snapshot(
        conn: &Connection,
        id: i64,
    ) -> (String, bool, String, Option<String>, String, Option<String>) {
        conn.query_row(
            "SELECT text, completed, created_at, completed_at, priority, due_date
             FROM todos WHERE id = ?1",
            [id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get::<_, i64>(1)? != 0,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            },
        )
        .unwrap()
    }

    #[test]
    fn undo_empty_journal_returns_none() {
        let conn = test_conn();
        assert!(undo_last(&conn).unwrap().is_none());
    }

    #[test]
    fn add_then_undo_removes_row_and_journal() {
        let conn = test_conn();
        add(&conn, "alpha");
        assert_eq!(task_count(&conn), 1);

        let outcome = undo_last(&conn).unwrap().expect("entry exists");
        assert_eq!(task_count(&conn), 0);
        assert_eq!(journal_count(&conn), 0);
        assert_eq!(outcome.op, UndoOp::Add);
        assert_eq!(
            outcome.entries,
            vec![UndoEntry {
                op: UndoOp::Add,
                text: Some("alpha".to_string()),
                completed: None,
            }]
        );
        assert_eq!(outcome.message(), "Reverted: task \"alpha\" removed.");
    }

    #[test]
    fn undo_is_lifo_across_batches() {
        let conn = test_conn();
        add(&conn, "alpha");
        add(&conn, "beta");

        undo_last(&conn).unwrap().unwrap();
        assert_eq!(task_count(&conn), 1);
        assert_eq!(journal_count(&conn), 1);

        undo_last(&conn).unwrap().unwrap();
        assert_eq!(task_count(&conn), 0);
        assert_eq!(journal_count(&conn), 0);
    }

    #[test]
    fn one_batch_add_reverts_all_tasks_at_once() {
        let conn = test_conn();
        let batch = next_batch(&conn).unwrap();
        for text in ["a", "b", "c"] {
            db::add_task(&conn, text, Priority::Medium, None, Repeat::None, batch).unwrap();
        }
        assert_eq!(task_count(&conn), 3);

        let outcome = undo_last(&conn).unwrap().expect("entry exists");
        assert_eq!(task_count(&conn), 0);
        assert_eq!(journal_count(&conn), 0);
        assert_eq!(outcome.message(), "Reverted: 3 tasks removed.");
    }

    #[test]
    fn undo_tolerates_already_deleted_row() {
        let conn = test_conn();
        add(&conn, "alpha");
        conn.execute("DELETE FROM todos", []).unwrap();

        let outcome = undo_last(&conn).unwrap().expect("journal entry still pops");
        assert_eq!(journal_count(&conn), 0);
        assert_eq!(outcome.message(), "Undone: task was already removed.");
    }

    #[test]
    fn next_batch_increments() {
        let conn = test_conn();
        assert_eq!(next_batch(&conn).unwrap(), 1);
        add(&conn, "alpha");
        assert_eq!(next_batch(&conn).unwrap(), 2);
        add(&conn, "beta");
        assert_eq!(next_batch(&conn).unwrap(), 3);
    }

    #[test]
    fn record_stores_before_image_when_provided() {
        let conn = test_conn();
        add(&conn, "alpha");
        let todo = crate::db::list_tasks(&conn, None, None, None, None, None, None)
            .unwrap()
            .swap_remove(0);
        record(&conn, 99, UndoOp::Add, todo.id, Some(&todo)).unwrap();

        let (text, completed, priority): (String, i64, String) = conn
            .query_row(
                "SELECT before_text, before_completed, before_priority FROM undo_log WHERE batch = 99",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(text, "alpha");
        assert_eq!(completed, 0);
        assert_eq!(priority, "medium");
    }

    #[test]
    fn done_then_undo_restores_pending_state() {
        let conn = test_conn();
        add(&conn, "alpha");
        let id = only_id(&conn);

        let batch = next_batch(&conn).unwrap();
        db::mark_completed(&conn, &id, batch).unwrap();
        let (done, at) = completion(&conn, id);
        assert!(done);
        assert!(at.is_some());

        let outcome = undo_last(&conn).unwrap().expect("entry exists");
        let (done, at) = completion(&conn, id);
        assert!(!done);
        assert!(at.is_none());
        assert_eq!(task_count(&conn), 1);
        assert_eq!(journal_count(&conn), 1);
        assert_eq!(outcome.message(), "Reverted: task \"alpha\" is now pending.");
    }

    #[test]
    fn undo_reverts_completion_before_addition() {
        let conn = test_conn();
        add(&conn, "alpha");
        let id = only_id(&conn);
        let batch = next_batch(&conn).unwrap();
        db::mark_completed(&conn, &id, batch).unwrap();

        let outcome = undo_last(&conn).unwrap().unwrap();
        assert_eq!(outcome.op, UndoOp::Done);
        assert_eq!(task_count(&conn), 1);
        assert!(!completion(&conn, id).0);

        let outcome = undo_last(&conn).unwrap().unwrap();
        assert_eq!(outcome.op, UndoOp::Add);
        assert_eq!(task_count(&conn), 0);
        assert_eq!(journal_count(&conn), 0);
    }

    #[test]
    fn undone_then_undo_restores_original_completed_at() {
        let conn = test_conn();
        add(&conn, "alpha");
        let id = only_id(&conn);

        let b1 = next_batch(&conn).unwrap();
        db::mark_completed(&conn, &id, b1).unwrap();
        let original_at = completion(&conn, id).1.unwrap();

        let b2 = next_batch(&conn).unwrap();
        db::mark_incomplete(&conn, &id, b2).unwrap();
        assert!(completion(&conn, id).1.is_none());

        let outcome = undo_last(&conn).unwrap().expect("entry exists");
        let (done, at) = completion(&conn, id);
        assert!(done);
        assert_eq!(at.as_deref(), Some(original_at.as_str()));
        assert_eq!(
            outcome.message(),
            "Reverted: task \"alpha\" is now completed."
        );
    }

    #[test]
    fn multi_done_batch_reverts_all_at_once() {
        let conn = test_conn();
        add(&conn, "a");
        add(&conn, "b");
        add(&conn, "c");
        let ids = all_ids(&conn);

        let batch = next_batch(&conn).unwrap();
        for id in &ids {
            db::mark_completed(&conn, id, batch).unwrap();
        }
        for id in &ids {
            assert!(completion(&conn, *id).0);
        }
        assert_eq!(journal_count(&conn), 6);

        let outcome = undo_last(&conn).unwrap().expect("entry exists");
        for id in &ids {
            assert!(!completion(&conn, *id).0);
        }
        assert_eq!(journal_count(&conn), 3);
        assert_eq!(outcome.message(), "Reverted: 3 tasks updated.");
    }

    #[test]
    fn done_on_completed_task_is_journaled_and_restores_timestamp() {
        let conn = test_conn();
        add(&conn, "alpha");
        let id = only_id(&conn);

        let b1 = next_batch(&conn).unwrap();
        db::mark_completed(&conn, &id, b1).unwrap();
        let first_at = completion(&conn, id).1.unwrap();

        std::thread::sleep(std::time::Duration::from_millis(5));
        let b2 = next_batch(&conn).unwrap();
        db::mark_completed(&conn, &id, b2).unwrap();
        let second_at = completion(&conn, id).1.unwrap();
        assert_ne!(first_at, second_at);

        let outcome = undo_last(&conn).unwrap().expect("journal entry exists");
        let (done, at) = completion(&conn, id);
        assert!(done);
        assert_eq!(at.unwrap(), first_at);
        assert_eq!(
            outcome.message(),
            "Reverted: task \"alpha\" is now completed."
        );
    }

    #[test]
    fn done_undo_tolerates_deleted_row() {
        let conn = test_conn();
        add(&conn, "alpha");
        let id = only_id(&conn);

        let batch = next_batch(&conn).unwrap();
        db::mark_completed(&conn, &id, batch).unwrap();
        conn.execute("DELETE FROM todos WHERE id = ?1", [id]).unwrap();

        let outcome = undo_last(&conn).unwrap().expect("journal entry pops");
        assert_eq!(journal_count(&conn), 1);
        assert_eq!(task_count(&conn), 0);
        assert_eq!(outcome.message(), "Undone: task was already removed.");
    }

    #[test]
    fn undone_on_pending_task_is_journaled_and_clean() {
        let conn = test_conn();
        add(&conn, "alpha");
        let id = only_id(&conn);

        let batch = next_batch(&conn).unwrap();
        db::mark_incomplete(&conn, &id, batch).unwrap();
        assert_eq!(journal_count(&conn), 2);

        let outcome = undo_last(&conn).unwrap().expect("entry exists");
        assert_eq!(outcome.op, UndoOp::Undone);
        let (done, at) = completion(&conn, id);
        assert!(!done);
        assert!(at.is_none());
        assert_eq!(journal_count(&conn), 1);
        assert_eq!(
            outcome.message(),
            "Reverted: task \"alpha\" is now pending."
        );
    }

    #[test]
    fn edit_then_undo_restores_original_text() {
        let conn = test_conn();
        add(&conn, "alpha");
        let id = only_id(&conn);
        edit(&conn, id, "omega");
        assert_eq!(text_of(&conn, id), "omega");

        let outcome = undo_last(&conn).unwrap().expect("entry exists");
        assert_eq!(outcome.op, UndoOp::Edit);
        assert_eq!(text_of(&conn, id), "alpha");
        assert_eq!(task_count(&conn), 1);
        assert_eq!(journal_count(&conn), 1);
        assert_eq!(outcome.message(), "Reverted: task text is now \"alpha\".");
    }

    #[test]
    fn undo_walks_full_history_edit_then_done_then_add() {
        let conn = test_conn();
        add(&conn, "alpha");
        let id = only_id(&conn);
        let batch = next_batch(&conn).unwrap();
        db::mark_completed(&conn, &id, batch).unwrap();
        edit(&conn, id, "omega");

        let outcome = undo_last(&conn).unwrap().unwrap();
        assert_eq!(outcome.op, UndoOp::Edit);
        assert_eq!(text_of(&conn, id), "alpha");
        assert!(completion(&conn, id).0);

        let outcome = undo_last(&conn).unwrap().unwrap();
        assert_eq!(outcome.op, UndoOp::Done);
        assert!(!completion(&conn, id).0);
        assert_eq!(task_count(&conn), 1);

        let outcome = undo_last(&conn).unwrap().unwrap();
        assert_eq!(outcome.op, UndoOp::Add);
        assert_eq!(task_count(&conn), 0);
        assert_eq!(journal_count(&conn), 0);
        assert!(undo_last(&conn).unwrap().is_none());
    }

    #[test]
    fn edit_undo_preserves_completion_state() {
        let conn = test_conn();
        add(&conn, "alpha");
        let id = only_id(&conn);
        let batch = next_batch(&conn).unwrap();
        db::mark_completed(&conn, &id, batch).unwrap();
        let at = completion(&conn, id).1.unwrap();

        edit(&conn, id, "omega");
        let outcome = undo_last(&conn).unwrap().expect("entry exists");
        assert_eq!(text_of(&conn, id), "alpha");
        let (done, restored_at) = completion(&conn, id);
        assert!(done);
        assert_eq!(restored_at.as_deref(), Some(at.as_str()));
        assert_eq!(outcome.message(), "Reverted: task text is now \"alpha\".");
    }

    #[test]
    fn edit_undo_tolerates_deleted_row() {
        let conn = test_conn();
        add(&conn, "alpha");
        let id = only_id(&conn);
        edit(&conn, id, "omega");
        conn.execute("DELETE FROM todos WHERE id = ?1", [id]).unwrap();

        let outcome = undo_last(&conn).unwrap().expect("journal pops");
        assert_eq!(journal_count(&conn), 1);
        assert_eq!(outcome.message(), "Undone: task was already removed.");
    }

    #[test]
    fn unchanged_text_edit_is_journaled_and_clean() {
        let conn = test_conn();
        add(&conn, "alpha");
        let id = only_id(&conn);
        edit(&conn, id, "alpha");
        assert_eq!(journal_count(&conn), 2);

        let outcome = undo_last(&conn).unwrap().unwrap();
        assert_eq!(outcome.op, UndoOp::Edit);
        assert_eq!(text_of(&conn, id), "alpha");
        assert_eq!(journal_count(&conn), 1);
        assert_eq!(outcome.message(), "Reverted: task text is now \"alpha\".");
    }

    #[test]
    fn two_edits_in_one_batch_revert_at_once() {
        let conn = test_conn();
        add(&conn, "a");
        add(&conn, "b");
        let ids = all_ids(&conn);

        let batch = next_batch(&conn).unwrap();
        db::update_task_text(&conn, &ids[0], "a2", batch).unwrap();
        db::update_task_text(&conn, &ids[1], "b2", batch).unwrap();

        let outcome = undo_last(&conn).unwrap().expect("entry exists");
        assert_eq!(text_of(&conn, ids[0]), "a");
        assert_eq!(text_of(&conn, ids[1]), "b");
        assert_eq!(outcome.message(), "Reverted: 2 tasks updated.");
    }

    #[test]
    fn edit_undo_leaves_add_entry_for_next_undo() {
        let conn = test_conn();
        add(&conn, "alpha");
        let id = only_id(&conn);
        edit(&conn, id, "omega");

        undo_last(&conn).unwrap().unwrap();
        assert_eq!(journal_count(&conn), 1);
        assert_eq!(task_count(&conn), 1);

        let outcome = undo_last(&conn).unwrap().unwrap();
        assert_eq!(outcome.op, UndoOp::Add);
        assert_eq!(task_count(&conn), 0);
        assert_eq!(journal_count(&conn), 0);
    }

    #[test]
    fn delete_then_undo_restores_full_row() {
        let conn = test_conn();
        add(&conn, "alpha");
        let id = only_id(&conn);
        let before = row_snapshot(&conn, id);

        delete(&conn, id);
        assert_eq!(task_count(&conn), 0);

        let outcome = undo_last(&conn).unwrap().expect("entry exists");
        assert_eq!(outcome.op, UndoOp::Delete);
        assert_eq!(row_snapshot(&conn, id), before);
        assert_eq!(journal_count(&conn), 1);
        assert_eq!(outcome.message(), "Reverted: task \"alpha\" restored.");
    }

    #[test]
    fn delete_undo_identity_cycle() {
        let conn = test_conn();
        add(&conn, "alpha");
        let id = only_id(&conn);
        delete(&conn, id);

        undo_last(&conn).unwrap().unwrap();
        assert_eq!(task_count(&conn), 1);
        assert_eq!(only_id(&conn), id);

        let outcome = undo_last(&conn).unwrap().unwrap();
        assert_eq!(outcome.op, UndoOp::Add);
        assert_eq!(task_count(&conn), 0);
        assert_eq!(journal_count(&conn), 0);
    }

    #[test]
    fn full_history_walk_add_done_edit_delete() {
        let conn = test_conn();
        add(&conn, "alpha");
        let id = only_id(&conn);
        let batch = next_batch(&conn).unwrap();
        db::mark_completed(&conn, &id, batch).unwrap();
        edit(&conn, id, "omega");
        delete(&conn, id);
        assert_eq!(task_count(&conn), 0);

        let outcome = undo_last(&conn).unwrap().unwrap();
        assert_eq!(outcome.op, UndoOp::Delete);
        assert_eq!(text_of(&conn, id), "omega");
        assert!(completion(&conn, id).0);

        let outcome = undo_last(&conn).unwrap().unwrap();
        assert_eq!(outcome.op, UndoOp::Edit);
        assert_eq!(text_of(&conn, id), "alpha");
        assert!(completion(&conn, id).0);

        let outcome = undo_last(&conn).unwrap().unwrap();
        assert_eq!(outcome.op, UndoOp::Done);
        assert!(!completion(&conn, id).0);

        let outcome = undo_last(&conn).unwrap().unwrap();
        assert_eq!(outcome.op, UndoOp::Add);
        assert_eq!(task_count(&conn), 0);
        assert!(undo_last(&conn).unwrap().is_none());
    }

    #[test]
    fn delete_c_batch_restores_exactly_completed_tasks() {
        let conn = test_conn();
        add(&conn, "a");
        add(&conn, "b");
        add(&conn, "c");
        add(&conn, "d");
        let ids = all_ids(&conn);

        let batch = next_batch(&conn).unwrap();
        for id in &ids[..3] {
            db::mark_completed(&conn, id, batch).unwrap();
        }

        let batch = next_batch(&conn).unwrap();
        db::delete_all_completed(&conn, batch).unwrap();
        assert_eq!(task_count(&conn), 1);

        let outcome = undo_last(&conn).unwrap().expect("entry exists");
        assert_eq!(task_count(&conn), 4);
        for id in &ids[..3] {
            assert!(completion(&conn, *id).0);
        }
        assert!(!completion(&conn, ids[3]).0);
        assert_eq!(outcome.message(), "Reverted: 3 tasks restored.");
    }

    #[test]
    fn multi_id_delete_in_one_batch_restores_all() {
        let conn = test_conn();
        add(&conn, "a");
        add(&conn, "b");
        add(&conn, "c");
        let ids = all_ids(&conn);

        let batch = next_batch(&conn).unwrap();
        for id in &ids {
            db::delete_task(&conn, id, batch).unwrap();
        }
        assert_eq!(task_count(&conn), 0);

        let outcome = undo_last(&conn).unwrap().expect("entry exists");
        assert_eq!(task_count(&conn), 3);
        for (id, expected) in ids.iter().zip(["a", "b", "c"]) {
            assert_eq!(text_of(&conn, *id), expected);
        }
        assert_eq!(outcome.message(), "Reverted: 3 tasks restored.");
    }

    #[test]
    fn delete_completed_task_undo_restores_completion_state() {
        let conn = test_conn();
        add(&conn, "alpha");
        let id = only_id(&conn);
        let batch = next_batch(&conn).unwrap();
        db::mark_completed(&conn, &id, batch).unwrap();
        let before = row_snapshot(&conn, id);
        assert!(before.1);

        delete(&conn, id);
        let outcome = undo_last(&conn).unwrap().expect("entry exists");
        assert_eq!(row_snapshot(&conn, id), before);
        assert_eq!(outcome.message(), "Reverted: task \"alpha\" restored.");
    }

    #[test]
    fn delete_no_ops_create_no_journal_entries() {
        let conn = test_conn();
        add(&conn, "alpha");
        assert_eq!(journal_count(&conn), 1);

        let batch = next_batch(&conn).unwrap();
        db::delete_task(&conn, &99, batch).unwrap();
        assert_eq!(journal_count(&conn), 1);

        let batch = next_batch(&conn).unwrap();
        db::delete_all_completed(&conn, batch).unwrap();
        assert_eq!(journal_count(&conn), 1);
        assert_eq!(task_count(&conn), 1);
    }

    #[test]
    fn delete_undo_skips_restore_when_id_already_exists() {
        let conn = test_conn();
        add(&conn, "alpha");
        let id = only_id(&conn);
        let mut todo = db::list_tasks(&conn, None, None, None, None, None, None)
            .unwrap()
            .swap_remove(0);
        todo.text = "ghost".to_string();

        let batch = next_batch(&conn).unwrap();
        record(&conn, batch, UndoOp::Delete, id, Some(&todo)).unwrap();

        let outcome = undo_last(&conn).unwrap().expect("journal pops");
        assert_eq!(journal_count(&conn), 1);
        assert_eq!(text_of(&conn, id), "alpha");
        assert_eq!(outcome.entries.len(), 1);
    }

    #[test]
    fn undo_n_pops_multiple_levels_in_order() {
        let conn = test_conn();
        add(&conn, "x");
        let id = only_id(&conn);
        add(&conn, "y");
        let batch = next_batch(&conn).unwrap();
        db::mark_completed(&conn, &id, batch).unwrap();
        delete(&conn, id);

        let outcomes = undo_n(&conn, 4).unwrap();
        let ops: Vec<UndoOp> = outcomes.iter().map(|o| o.op).collect();
        assert_eq!(
            ops,
            vec![UndoOp::Delete, UndoOp::Done, UndoOp::Add, UndoOp::Add]
        );
        assert_eq!(outcomes[0].message(), "Reverted: task \"x\" restored.");
        assert_eq!(outcomes[1].message(), "Reverted: task \"x\" is now pending.");
        assert_eq!(outcomes[2].message(), "Reverted: task \"y\" removed.");
        assert_eq!(outcomes[3].message(), "Reverted: task \"x\" removed.");
        assert_eq!(task_count(&conn), 0);
        assert_eq!(journal_count(&conn), 0);
    }

    #[test]
    fn undo_n_stops_at_stack_bottom() {
        let conn = test_conn();
        add(&conn, "a");
        add(&conn, "b");

        let outcomes = undo_n(&conn, 5).unwrap();
        assert_eq!(outcomes.len(), 2);
        assert_eq!(task_count(&conn), 0);
        assert_eq!(journal_count(&conn), 0);
    }

    #[test]
    fn undo_n_empty_stack_returns_no_outcomes() {
        let conn = test_conn();
        let outcomes = undo_n(&conn, 3).unwrap();
        assert!(outcomes.is_empty());
    }

    #[test]
    fn undo_n_partial_count_leaves_remainder() {
        let conn = test_conn();
        add(&conn, "a");
        add(&conn, "b");
        add(&conn, "c");

        let outcomes = undo_n(&conn, 1).unwrap();
        assert_eq!(outcomes.len(), 1);
        assert_eq!(task_count(&conn), 2);
        assert_eq!(journal_count(&conn), 2);
    }

    fn tomorrow() -> String {
        chrono::NaiveDate::parse_from_str(&crate::filter::today(), "%Y-%m-%d")
            .unwrap()
            .checked_add_days(chrono::Days::new(1))
            .unwrap()
            .format("%Y-%m-%d")
            .to_string()
    }

    #[test]
    fn completing_daily_task_spawns_next_occurrence() {
        let conn = test_conn();
        let batch = next_batch(&conn).unwrap();
        db::add_task(&conn, "exercise", Priority::Medium, None, Repeat::Daily, batch).unwrap();
        let id = only_id(&conn);

        let batch = next_batch(&conn).unwrap();
        db::mark_completed(&conn, &id, batch).unwrap();

        assert_eq!(task_count(&conn), 2);

        let (completed, repeat): (i64, String) = conn
            .query_row(
                "SELECT completed, repeat FROM todos WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(completed, 1);
        assert_eq!(repeat, "daily");

        let (copy_completed, due, copy_repeat): (i64, Option<String>, String) = conn
            .query_row(
                "SELECT completed, due_date, repeat FROM todos WHERE id != ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(copy_completed, 0);
        assert_eq!(due, Some(tomorrow()));
        assert_eq!(copy_repeat, "daily");

        let entries: i64 = conn
            .query_row("SELECT COUNT(*) FROM undo_log WHERE batch = ?1", [batch], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(entries, 2);
        assert_eq!(journal_count(&conn), 3);
    }

    #[test]
    fn completing_non_recurring_task_does_not_spawn() {
        let conn = test_conn();
        add(&conn, "plain");
        let id = only_id(&conn);

        let batch = next_batch(&conn).unwrap();
        db::mark_completed(&conn, &id, batch).unwrap();

        assert_eq!(task_count(&conn), 1);
        assert_eq!(journal_count(&conn), 2);
    }

    #[test]
    fn completing_already_completed_task_does_not_respawn() {
        let conn = test_conn();
        let batch = next_batch(&conn).unwrap();
        db::add_task(&conn, "exercise", Priority::Medium, None, Repeat::Daily, batch).unwrap();
        let id = only_id(&conn);

        let batch = next_batch(&conn).unwrap();
        db::mark_completed(&conn, &id, batch).unwrap();
        assert_eq!(task_count(&conn), 2);

        let batch = next_batch(&conn).unwrap();
        db::mark_completed(&conn, &id, batch).unwrap();
        assert_eq!(task_count(&conn), 2);
        assert_eq!(journal_count(&conn), 4);
    }

    #[test]
    fn undone_recurring_task_does_not_spawn() {
        let conn = test_conn();
        let batch = next_batch(&conn).unwrap();
        db::add_task(&conn, "exercise", Priority::Medium, None, Repeat::Daily, batch).unwrap();
        let id = only_id(&conn);

        let batch = next_batch(&conn).unwrap();
        db::mark_completed(&conn, &id, batch).unwrap();
        let batch = next_batch(&conn).unwrap();
        db::mark_incomplete(&conn, &id, batch).unwrap();

        assert_eq!(task_count(&conn), 2);
    }

    #[test]
    fn undo_completion_with_spawn_reverts_both_in_one_undo() {
        let conn = test_conn();
        let batch = next_batch(&conn).unwrap();
        db::add_task(&conn, "exercise", Priority::Medium, None, Repeat::Daily, batch).unwrap();
        let id = only_id(&conn);
        let batch = next_batch(&conn).unwrap();
        db::mark_completed(&conn, &id, batch).unwrap();
        assert_eq!(task_count(&conn), 2);

        let outcome = undo_last(&conn).unwrap().expect("journal pops");

        assert_eq!(task_count(&conn), 1);
        assert_eq!(journal_count(&conn), 1);
        let (row_id, completed, repeat): (i64, i64, String) = conn
            .query_row("SELECT id, completed, repeat FROM todos", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap();
        assert_eq!(row_id, id);
        assert_eq!(completed, 0);
        assert_eq!(repeat, "daily");
        assert_eq!(
            outcome.message(),
            "Reverted: task \"exercise\" is now pending."
        );
    }

    #[test]
    fn delete_undo_restores_repeat() {
        let conn = test_conn();
        let batch = next_batch(&conn).unwrap();
        db::add_task(&conn, "gym", Priority::High, Some("2026-10-05"), Repeat::Weekly, batch)
            .unwrap();
        let id = only_id(&conn);

        let batch = next_batch(&conn).unwrap();
        db::delete_task(&conn, &id, batch).unwrap();
        assert_eq!(task_count(&conn), 0);

        undo_last(&conn).unwrap().expect("journal pops");

        let (repeat, priority, due, completed): (String, String, Option<String>, i64) = conn
            .query_row("SELECT repeat, priority, due_date, completed FROM todos", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .unwrap();
        assert_eq!(repeat, "weekly");
        assert_eq!(priority, "high");
        assert_eq!(due, Some("2026-10-05".to_string()));
        assert_eq!(completed, 0);
    }
}
