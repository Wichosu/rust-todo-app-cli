use chrono::{DateTime, Local};
use std::env;

use crate::todo::Priority;
mod db;
mod todo;

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    let conn = db::connect()?;

    if args.len() < 2 {
        println!("Usage: todo <command>");
        return Ok(());
    }

    match args[1].as_str() {
        "list" => {
            let completed = match args.get(2).map(|v| v.as_str()) {
                Some("--completed") => Some(true),
                Some("--pending") => Some(false),
                _ => None,
            };

            let priority = args
                .iter()
                .position(|arg| arg == "--priority")
                .and_then(|i| args.get(i + 1))
                .map(|v| v.parse::<Priority>())
                .transpose()?;

            let due_today = args.iter().any(|arg| arg == "--due-today");
            let overdue = args.iter().any(|arg| arg == "--overdue");

            let due_before = args
                .iter()
                .position(|arg| arg == "--due-before")
                .and_then(|i| args.get(i + 1))
                .map(|s| s.as_str());

            let due_after = args
                .iter()
                .position(|arg| arg == "--due-after")
                .and_then(|i| args.get(i + 1))
                .map(|s| s.as_str());

            let completed = match (completed, priority, due_today, overdue) {
                (None, Some(_), _, _) => Some(false),
                (None, _, true, _) => Some(false),
                (None, _, _, true) => Some(false),
                _ => completed,
            };

            let due_date = if due_today {
                Some(Local::now().format("%Y-%m-%d").to_string())
            } else {
                None
            };

            let overdue_before = if overdue {
                Some(Local::now().format("%Y-%m-%d").to_string())
            } else {
                None
            };

            let todos = db::list_tasks(&conn, completed, priority, due_date.as_deref(), overdue_before.as_deref(), due_before, due_after)?;

            println!("List of todos: ");

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
                }
            }
        }
        "add" => {
            if args.len() < 3 {
                println!("Usage: todo add <tasks...> [options]");
                return Ok(());
            }

            let mut priority = Priority::Medium;

            if let Some(index) = args.iter().position(|arg| arg == "--priority") {
                if let Some(value) = args.get(index + 1) {
                    priority = value.parse()?;
                }
            }

            let due_date = args
                .iter()
                .position(|arg| arg == "--due")
                .and_then(|i| args.get(i + 1))
                .map(|s| s.as_str());

            for todo in &args[2..] {
                if todo.starts_with("--") {
                    break;
                }

                match db::add_task(&conn, &todo.to_string(), priority, due_date) {
                    Ok(()) => println!("Task successfully added!"),
                    Err(_) => println!("Something went wrong!"),
                }
            }
        }
        "delete" => {
            if args.len() < 3 {
                println!("Usage: todo delete <index_of_task>");
                return Ok(());
            }

            for index in &args[2..] {
                if index == "-c" {
                    if let Err(e) = db::delete_all_completed(&conn) {
                        println!("Error: {}", e);
                    } else {
                        println!("Deleted all completed tasks!");
                    }
                    return Ok(());
                }

                if let Ok(id) = index.parse::<i64>() {
                    if let Err(e) = db::delete_task(&conn, &id) {
                        println!("Error: {}", e);
                    } else {
                        println!("Task successfully deleted!");
                    }
                } else {
                    println!("Invalid index: {}", index);
                }
            }
        }
        "done" => {
            if args.len() < 3 {
                println!("Usage: todo done <index_of_task>");
                return Ok(());
            }

            for index in &args[2..] {
                if let Ok(id) = index.parse::<i64>() {
                    if let Err(e) = db::mark_completed(&conn, &id) {
                        println!("Error: {}", e);
                    } else {
                        println!("Task marked as completed!");
                    }
                } else {
                    println!("Invalid index: {}", index);
                }
            }
        }
        "undone" => {
            if args.len() < 3 {
                println!("Usage: todo undone <index_of_task>");
                return Ok(());
            }

            for index in &args[2..] {
                if let Ok(id) = index.parse::<i64>() {
                    if let Err(e) = db::mark_incomplete(&conn, &id) {
                        println!("Error: {}", e);
                    } else {
                        println!("Task marked as incomplete!");
                    }
                } else {
                    println!("Invalid index: {}", index);
                }
            }
        }
        "help" => {
            println!("List of commands:");
            println!("list -> shows the list of current tasks. Options: --completed, --pending, --priority <level>, --due-today, --overdue, --due-before <YYYY-MM-DD>, --due-after <YYYY-MM-DD>");
            println!("add -> adds new tasks. Usage: todo add <tasks...> [--priority low|medium|high] [--due YYYY-MM-DD]");
            println!("delete -> deletes tasks. Usage: todo delete <index_of_tasks...>");
            println!("done -> checks a task. Usage todo done <index_of_tasks...>");
            println!("undone -> unchecks a task. Usage todo undone <index_of_tasks...>");
        }
        _ => println!("Unknown command. use \"todo help\" to show available commands"),
    }

    Ok(())
}

fn main() {
    if let Err(err) = run() {
        eprintln!("Error: {}", err);
    }
}
