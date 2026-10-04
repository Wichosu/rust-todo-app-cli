use std::error::Error;

use chrono::{DateTime, Local};
use rusqlite::Connection;

use crate::db;
use crate::export;
use crate::filter::today;
use crate::todo::{Priority, Repeat, Todo};
use crate::undo;

type CmdResult = Result<(), Box<dyn Error>>;

pub fn run(conn: &Connection, args: &[String]) -> CmdResult {
    if args.len() < 2 {
        println!("Usage: todo <command>");
        return Ok(());
    }

    match args[1].as_str() {
        "list" => list(conn, args),
        "add" => add(conn, args),
        "delete" => delete(conn, args),
        "done" => done(conn, args),
        "undone" => undone(conn, args),
        "undo" => undo(conn, args),
        "search" => search(conn, args),
        "export" => export(conn, args),
        "import" => import(conn, args),
        "help" => {
            help();
            Ok(())
        }
        _ => {
            println!("Unknown command. use \"todo help\" to show available commands");
            Ok(())
        }
    }
}

fn flag(args: &[String], name: &str) -> bool {
    args.iter().any(|arg| arg == name)
}

fn opt_value<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
}

fn parse_priority(args: &[String]) -> Result<Option<Priority>, Box<dyn Error>> {
    Ok(opt_value(args, "--priority")
        .map(|v| v.parse::<Priority>())
        .transpose()?)
}

fn parse_repeat(args: &[String]) -> Result<Option<Repeat>, Box<dyn Error>> {
    Ok(opt_value(args, "--repeat")
        .map(|v| v.parse::<Repeat>())
        .transpose()?)
}

fn date_args(due_today: bool, overdue: bool) -> (Option<String>, Option<String>) {
    let today = today();
    (
        due_today.then(|| today.clone()),
        overdue.then_some(today),
    )
}

fn print_todos(todos: &[Todo]) -> CmdResult {
    for todo in todos {
        if todo.completed {
            println!("{:?}.- [X] {:?}", todo.id, todo.text);
            println!(
                "\tCreated: {}",
                todo.created_at
                    .parse::<DateTime<chrono::Utc>>()?
                    .with_timezone(&Local)
                    .format("%b %d, %Y %H:%M")
            );
            if let Some(timestamp) = &todo.completed_at {
                let dt = timestamp
                    .parse::<DateTime<chrono::Utc>>()?
                    .with_timezone(&Local);
                println!("\tCompleted: {}", dt.format("%b %d, %Y %H:%M"));
            }
            if let Some(due) = &todo.due_date {
                println!("\tDue: {}", due);
            }
            if todo.repeat != Repeat::None {
                println!("\tRepeat: {}", todo.repeat);
            }
        } else {
            println!("{:?}.- [{}] {:?}", todo.id, todo.priority, todo.text);
            println!(
                "\tCreated: {}",
                todo.created_at
                    .parse::<DateTime<chrono::Utc>>()?
                    .with_timezone(&Local)
                    .format("%b %d, %Y %H:%M")
            );
            if let Some(due) = &todo.due_date {
                println!("\tDue: {}", due);
            }
            if todo.repeat != Repeat::None {
                println!("\tRepeat: {}", todo.repeat);
            }
        }
    }
    Ok(())
}

fn run_over_ids(
    conn: &Connection,
    args: &[String],
    op: fn(&Connection, &i64, i64) -> rusqlite::Result<()>,
    success_msg: &str,
) -> CmdResult {
    let batch = undo::next_batch(conn)?;
    for index in &args[2..] {
        if let Ok(id) = index.parse::<i64>() {
            if let Err(e) = op(conn, &id, batch) {
                println!("Error: {}", e);
            } else {
                println!("{}", success_msg);
            }
        } else {
            println!("Invalid index: {}", index);
        }
    }
    Ok(())
}

fn list(conn: &Connection, args: &[String]) -> CmdResult {
    let completed = match args.get(2).map(|v| v.as_str()) {
        Some("--completed") => Some(true),
        Some("--pending") => Some(false),
        _ => None,
    };

    let priority = parse_priority(args)?;

    let due_today = flag(args, "--due-today");
    let overdue = flag(args, "--overdue");

    let due_before = opt_value(args, "--due-before");
    let due_after = opt_value(args, "--due-after");

    let completed = match (completed, priority, due_today, overdue) {
        (None, Some(_), _, _) => Some(false),
        (None, _, true, _) => Some(false),
        (None, _, _, true) => Some(false),
        _ => completed,
    };

    let (due_date, overdue_before) = date_args(due_today, overdue);

    let todos = db::list_tasks(
        conn,
        completed,
        priority,
        due_date.as_deref(),
        overdue_before.as_deref(),
        due_before,
        due_after,
    )?;

    println!("List of todos: ");
    print_todos(&todos)
}

fn add(conn: &Connection, args: &[String]) -> CmdResult {
    if args.len() < 3 {
        println!("Usage: todo add <tasks...> [options]");
        return Ok(());
    }

    let mut priority = Priority::Medium;
    if let Some(value) = opt_value(args, "--priority") {
        priority = value.parse()?;
    }

    let due_date = opt_value(args, "--due");
    let repeat = parse_repeat(args)?.unwrap_or_default();

    let batch = undo::next_batch(conn)?;

    for todo in &args[2..] {
        if todo.starts_with("--") {
            break;
        }

        match db::add_task(conn, todo, priority, due_date, repeat, batch) {
            Ok(()) => println!("Task successfully added!"),
            Err(_) => println!("Something went wrong!"),
        }
    }
    Ok(())
}

fn export(conn: &Connection, args: &[String]) -> CmdResult {
    let todos = db::list_tasks(conn, None, None, None, None, None, None)?;
    let json = export::to_json(&todos)?;

    match args.get(2) {
        None => println!("{}", json),
        Some(path) => {
            std::fs::write(path, &json)?;
            println!("Exported {} tasks to {}.", todos.len(), path);
        }
    }
    Ok(())
}

fn import(conn: &Connection, args: &[String]) -> CmdResult {
    let path = match args.get(2) {
        Some(path) if !path.starts_with("--") => path,
        _ => {
            println!("Usage: todo import <path> [--replace]");
            return Ok(());
        }
    };

    let json = match std::fs::read_to_string(path) {
        Ok(json) => json,
        Err(err) => {
            println!("Import error: cannot read {}: {}", path, err);
            return Ok(());
        }
    };

    let todos = match export::parse_and_validate(&json) {
        Ok(todos) => todos,
        Err(err) => {
            println!("Import error: {}", err);
            return Ok(());
        }
    };

    let batch = undo::next_batch(conn)?;
    if flag(args, "--replace") {
        let (replaced, imported) = db::replace_all_tasks(conn, &todos, batch)?;
        println!("Replaced {} tasks with {} imported.", replaced, imported);
    } else {
        let inserted = db::import_tasks(conn, &todos, batch)?;
        println!("Imported {} tasks.", inserted);
    }
    Ok(())
}

fn undo_count(args: &[String]) -> Result<usize, String> {
    match args.get(2) {
        None => Ok(1),
        Some(raw) => match raw.parse::<usize>() {
            Ok(n) if n >= 1 => Ok(n),
            _ => Err(raw.clone()),
        },
    }
}

fn undo(conn: &Connection, args: &[String]) -> CmdResult {
    let count = match undo_count(args) {
        Ok(count) => count,
        Err(raw) => {
            println!("Invalid count: {}", raw);
            return Ok(());
        }
    };

    let outcomes = undo::undo_n(conn, count)?;
    if outcomes.is_empty() {
        println!("Nothing to undo.");
    } else {
        for outcome in &outcomes {
            println!("{}", outcome.message());
        }
    }
    Ok(())
}

fn delete(conn: &Connection, args: &[String]) -> CmdResult {
    if args.len() < 3 {
        println!("Usage: todo delete <index_of_task>");
        return Ok(());
    }

    let batch = undo::next_batch(conn)?;

    for index in &args[2..] {
        if index == "-c" {
            if let Err(e) = db::delete_all_completed(conn, batch) {
                println!("Error: {}", e);
            } else {
                println!("Deleted all completed tasks!");
            }
            return Ok(());
        }

        if let Ok(id) = index.parse::<i64>() {
            if let Err(e) = db::delete_task(conn, &id, batch) {
                println!("Error: {}", e);
            } else {
                println!("Task successfully deleted!");
            }
        } else {
            println!("Invalid index: {}", index);
        }
    }
    Ok(())
}

fn done(conn: &Connection, args: &[String]) -> CmdResult {
    if args.len() < 3 {
        println!("Usage: todo done <index_of_task>");
        return Ok(());
    }
    run_over_ids(conn, args, db::mark_completed, "Task marked as completed!")
}

fn undone(conn: &Connection, args: &[String]) -> CmdResult {
    if args.len() < 3 {
        println!("Usage: todo undone <index_of_task>");
        return Ok(());
    }
    run_over_ids(conn, args, db::mark_incomplete, "Task marked as incomplete!")
}

fn help() {
    println!("List of commands:");
    println!("list -> shows the list of current tasks. Options: --completed, --pending, --priority <level>, --due-today, --overdue, --due-before <YYYY-MM-DD>, --due-after <YYYY-MM-DD>");
    println!("add -> adds new tasks. Usage: todo add <tasks...> [--priority low|medium|high] [--due YYYY-MM-DD] [--repeat none|daily|weekly|monthly]");
    println!("delete -> deletes tasks. Usage: todo delete <index_of_tasks...>");
    println!("done -> checks a task. Usage todo done <index_of_tasks...>");
    println!("undone -> unchecks a task. Usage todo undone <index_of_tasks...>");
    println!("undo -> reverts the recent action(s). Usage: todo undo [count]");
    println!("search -> searches tasks by text. Usage: todo search <query> [--pending|--completed] [--priority <level>] [--due-today] [--overdue]");
    println!("export -> exports tasks as JSON. Usage: todo export [path]");
    println!("import -> imports tasks from JSON. Usage: todo import <path> [--replace]");
    println!("ui -> launches the terminal UI");
}

fn search(conn: &Connection, args: &[String]) -> CmdResult {
    if args.len() < 3 {
        println!("Usage: todo search <query> [--pending|--completed] [--priority <level>] [--due-today] [--overdue]");
        return Ok(());
    }

    let query = &args[2];

    let completed = if flag(args, "--pending") {
        Some(false)
    } else if flag(args, "--completed") {
        Some(true)
    } else {
        None
    };

    let priority = parse_priority(args)?;

    let due_today = flag(args, "--due-today");
    let overdue = flag(args, "--overdue");

    let (due_date, overdue_before) = date_args(due_today, overdue);

    let todos = db::search_tasks(
        conn,
        query,
        completed,
        priority,
        due_date.as_deref(),
        overdue_before.as_deref(),
    )?;

    if todos.is_empty() {
        println!("No tasks found matching \"{}\"", query);
    } else {
        println!("Search results for \"{}\":", query);
        print_todos(&todos)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn flag_detects_presence() {
        let a = args(&["todo", "list", "--overdue"]);
        assert!(flag(&a, "--overdue"));
        assert!(!flag(&a, "--due-today"));
    }

    #[test]
    fn opt_value_returns_following_arg() {
        let a = args(&["todo", "list", "--due-before", "2026-01-01"]);
        assert_eq!(opt_value(&a, "--due-before"), Some("2026-01-01"));
        assert_eq!(opt_value(&a, "--due-after"), None);
    }

    #[test]
    fn opt_value_missing_value_returns_none() {
        let a = args(&["todo", "list", "--due-before"]);
        assert_eq!(opt_value(&a, "--due-before"), None);
    }

    #[test]
    fn parse_priority_valid_and_missing() {
        let a = args(&["todo", "list", "--priority", "high"]);
        assert_eq!(parse_priority(&a).unwrap(), Some(Priority::High));

        let a = args(&["todo", "list"]);
        assert_eq!(parse_priority(&a).unwrap(), None);
    }

    #[test]
    fn parse_priority_invalid_errors() {
        let a = args(&["todo", "list", "--priority", "bogus"]);
        assert!(parse_priority(&a).is_err());
    }

    #[test]
    fn date_args_maps_flags() {
        let (due, overdue) = date_args(true, false);
        assert!(due.is_some());
        assert!(overdue.is_none());

        let (due, overdue) = date_args(false, true);
        assert!(due.is_none());
        assert!(overdue.is_some());

        let (due, overdue) = date_args(false, false);
        assert!(due.is_none() && overdue.is_none());
    }

    #[test]
    fn undo_count_defaults_to_one() {
        let a = args(&["todo", "undo"]);
        assert_eq!(undo_count(&a), Ok(1));
    }

    #[test]
    fn undo_count_parses_positive_integer() {
        let a = args(&["todo", "undo", "3"]);
        assert_eq!(undo_count(&a), Ok(3));
    }

    #[test]
    fn undo_count_rejects_zero() {
        let a = args(&["todo", "undo", "0"]);
        assert_eq!(undo_count(&a), Err("0".to_string()));
    }

    #[test]
    fn undo_count_rejects_non_numeric() {
        let a = args(&["todo", "undo", "abc"]);
        assert_eq!(undo_count(&a), Err("abc".to_string()));
    }

    #[test]
    fn undo_count_rejects_mixed_alphanumeric() {
        let a = args(&["todo", "undo", "2x"]);
        assert_eq!(undo_count(&a), Err("2x".to_string()));
    }

    #[test]
    fn parse_repeat_defaults_to_none() {
        let a = args(&["todo", "add", "x"]);
        assert_eq!(parse_repeat(&a).unwrap(), None);
        assert_eq!(parse_repeat(&a).unwrap().unwrap_or_default(), Repeat::None);
    }

    #[test]
    fn parse_repeat_accepts_valid_values() {
        let a = args(&["todo", "add", "x", "--repeat", "weekly"]);
        assert_eq!(parse_repeat(&a).unwrap(), Some(Repeat::Weekly));
    }

    #[test]
    fn parse_repeat_rejects_invalid_value() {
        let a = args(&["todo", "add", "x", "--repeat", "yearly"]);
        let err = parse_repeat(&a).unwrap_err().to_string();
        assert!(err.contains("yearly"));
        assert!(err.contains("none, daily, weekly, monthly"));
    }
}
