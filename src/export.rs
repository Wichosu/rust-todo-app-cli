use chrono::DateTime;
use serde::{Deserialize, Serialize};

use crate::todo::Todo;

pub const FORMAT: &str = "todo-app-cli";
pub const VERSION: u32 = 1;

#[derive(Serialize)]
struct ExportFile<'a> {
    format: &'a str,
    version: u32,
    exported_at: String,
    todos: &'a [Todo],
}

#[derive(Deserialize)]
struct ImportFile {
    format: String,
    version: u32,
    #[serde(default)]
    #[allow(dead_code)]
    exported_at: Option<String>,
    todos: Vec<Todo>,
}

pub fn to_json(todos: &[Todo]) -> Result<String, String> {
    let mut sorted = todos.to_vec();
    sorted.sort_by_key(|t| t.id);

    let file = ExportFile {
        format: FORMAT,
        version: VERSION,
        exported_at: chrono::Utc::now().to_rfc3339(),
        todos: sorted.as_slice(),
    };
    serde_json::to_string_pretty(&file).map_err(|e| e.to_string())
}

pub fn parse_and_validate(json: &str) -> Result<Vec<Todo>, String> {
    let file: ImportFile =
        serde_json::from_str(json).map_err(|e| format!("cannot parse export file: {e}"))?;

    if file.format != FORMAT {
        return Err(format!(
            "unexpected format \"{}\" (expected \"{}\")",
            file.format, FORMAT
        ));
    }
    if file.version != VERSION {
        return Err(format!("Unsupported export version: {}", file.version));
    }

    let mut seen_ids = std::collections::HashSet::new();
    for (i, todo) in file.todos.iter().enumerate() {
        if todo.id < 1 {
            return Err(format!(
                "todos[{i}].id must be a positive integer ({})",
                todo.id
            ));
        }
        if !seen_ids.insert(todo.id) {
            return Err(format!("todos[{i}].id {} is duplicated", todo.id));
        }
        if DateTime::parse_from_rfc3339(&todo.created_at).is_err() {
            return Err(format!(
                "todos[{i}].created_at is not valid RFC3339 (\"{}\")",
                todo.created_at
            ));
        }
        if let Some(completed_at) = &todo.completed_at
            && DateTime::parse_from_rfc3339(completed_at).is_err()
        {
            return Err(format!(
                "todos[{i}].completed_at is not valid RFC3339 (\"{completed_at}\")"
            ));
        }
    }

    Ok(file.todos)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::todo::{Priority, Repeat};
    use crate::undo;
    use rusqlite::Connection;

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        db::init_schema(&conn).unwrap();
        conn
    }

    fn sample_todos() -> Vec<Todo> {
        let conn = test_conn();
        let batch = undo::next_batch(&conn).unwrap();
        db::add_task(&conn, "exercise", Priority::Medium, None, Repeat::Daily, batch).unwrap();
        db::add_task(&conn, "laundry", Priority::High, Some("2026-10-05"), Repeat::Weekly, batch)
            .unwrap();
        let id = 2;
        db::mark_completed(&conn, &id, batch).unwrap();
        db::list_tasks(&conn, None, None, None, None, None, None).unwrap()
    }

    fn one_todo_json() -> String {
        let todos = sample_todos();
        to_json(&todos).unwrap()
    }

    #[test]
    fn round_trip_preserves_all_fields() {
        let todos = sample_todos();
        let json = to_json(&todos).unwrap();
        let restored = parse_and_validate(&json).unwrap();

        assert_eq!(restored.len(), todos.len());
        for (a, b) in restored.iter().zip(todos.iter()) {
            assert_eq!(a.id, b.id);
            assert_eq!(a.text, b.text);
            assert_eq!(a.completed, b.completed);
            assert_eq!(a.created_at, b.created_at);
            assert_eq!(a.completed_at, b.completed_at);
            assert_eq!(a.priority, b.priority);
            assert_eq!(a.due_date, b.due_date);
            assert_eq!(a.repeat, b.repeat);
        }
    }

    #[test]
    fn json_shape_has_format_version_and_bools() {
        let json = one_todo_json();
        assert!(json.contains("\"format\": \"todo-app-cli\""));
        assert!(json.contains("\"version\": 1"));
        assert!(json.contains("\"completed\": false"));
        assert!(json.contains("\"completed\": true"));
        assert!(json.contains("\"priority\": \"high\""));
        assert!(json.contains("\"repeat\": \"daily\""));
        assert!(json.contains("\"due_date\": null"));
        assert!(json.contains("\"exported_at\""));
    }

    #[test]
    fn parse_rejects_wrong_format() {
        let json = one_todo_json().replace("todo-app-cli", "other-app");
        let err = parse_and_validate(&json).unwrap_err();
        assert!(err.contains("unexpected format"));
    }

    #[test]
    fn parse_rejects_unsupported_version() {
        let json = one_todo_json().replace("\"version\": 1", "\"version\": 2");
        let err = parse_and_validate(&json).unwrap_err();
        assert_eq!(err, "Unsupported export version: 2");
    }

    #[test]
    fn parse_rejects_garbage_json() {
        let err = parse_and_validate("{not json").unwrap_err();
        assert!(err.contains("cannot parse export file"));
    }

    #[test]
    fn parse_rejects_bad_created_at_with_index() {
        let todos = sample_todos();
        let json = to_json(&todos).unwrap().replace(&todos[0].created_at, "banana");
        let err = parse_and_validate(&json).unwrap_err();
        assert!(err.contains("todos["));
        assert!(err.contains(".created_at"));
        assert!(err.contains("banana"));
    }

    #[test]
    fn parse_rejects_zero_id() {
        let json = one_todo_json().replace("\"id\": 1,", "\"id\": 0,");
        let err = parse_and_validate(&json).unwrap_err();
        assert!(err.contains("todos[0].id"));
        assert!(err.contains("positive"));
    }

    #[test]
    fn parse_rejects_duplicate_ids() {
        let mut todos = sample_todos();
        let dup = Todo {
            id: todos[0].id,
            ..todos[1].clone()
        };
        todos.push(dup);
        let json = to_json(&todos).unwrap();
        let err = parse_and_validate(&json).unwrap_err();
        assert!(err.contains("duplicated"));
    }

    #[test]
    fn parse_rejects_unknown_priority() {
        let json = one_todo_json().replace("\"priority\": \"high\"", "\"priority\": \"urgent\"");
        let err = parse_and_validate(&json).unwrap_err();
        assert!(err.contains("urgent"));
    }

    #[test]
    fn parse_accepts_empty_todos() {
        let json = r#"{"format":"todo-app-cli","version":1,"exported_at":"2026-10-04T00:00:00Z","todos":[]}"#;
        let todos = parse_and_validate(json).unwrap();
        assert!(todos.is_empty());
    }

    #[test]
    fn parse_ignores_unknown_fields_and_missing_exported_at() {
        let json = r#"{"format":"todo-app-cli","version":1,"todos":[],"extra":"future"}"#;
        let todos = parse_and_validate(json).unwrap();
        assert!(todos.is_empty());
    }
}
