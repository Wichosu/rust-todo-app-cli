use chrono::{DateTime, Local};
use std::env;

use crate::todo::Priority;
mod db;
mod todo;

fn main() {
    let args: Vec<String> = env::args().collect();

    let conn = db::connect().unwrap();

    if args.len() < 2 {
        println!("Usage: todo <command>");
        return;
    }

    match args[1].as_str() {
        "list" => {
            let todos = db::list_tasks(&conn).unwrap();

            println!("List of todos: ");

            for todo in todos {
                if todo.completed {
                    println!("{:?}.- [X] {:?}", todo.id, todo.text);
                    println!(
                        "\tCreated: {}",
                        todo.created_at
                            .parse::<DateTime<chrono::Utc>>()
                            .unwrap()
                            .with_timezone(&Local)
                            .format("%b %d, %Y %H:%M")
                    );
                    if let Some(timestamp) = &todo.completed_at {
                        let dt = timestamp
                            .parse::<DateTime<chrono::Utc>>()
                            .unwrap()
                            .with_timezone(&Local);
                        println!("\tCompleted: {}", dt.format("%b %d, %Y %H:%M"));
                    }
                } else {
                    println!("{:?}.- [{}] {:?}", todo.id, todo.priority, todo.text);
                    println!(
                        "\tCreated: {}",
                        todo.created_at
                            .parse::<DateTime<chrono::Utc>>()
                            .unwrap()
                            .with_timezone(&Local)
                            .format("%b %d, %Y %H:%M")
                    );
                }
            }
        }
        "add" => {
            if args.len() < 3 {
                println!("Usage: todo add <tasks...> [options]");
                return;
            }

            let mut priority = Priority::Medium;

            if let Some(index) = args.iter().position(|arg| arg == "--priority") {
                if let Some(value) = args.get(index + 1) {
                    priority = value.parse().unwrap();
                }
            }

            for todo in &args[2..] {
                if todo.starts_with("--") {
                    break;
                }

                match db::add_task(&conn, &todo.to_string(), priority) {
                    Ok(()) => println!("Task successfully added!"),
                    Err(_) => println!("Something went wrong!"),
                }
            }
        }
        "delete" => {
            if args.len() < 3 {
                println!("Usage: todo delete <index_of_task>");
                return;
            }

            for index in &args[2..] {
                if index == "-c" {
                    if let Err(e) = db::delete_all_completed(&conn) {
                        println!("Error: {}", e);
                    } else {
                        println!("Deleted all completed tasks!");
                    }
                    return;
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
                return;
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
                return;
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
            println!("list -> shows the list of current tasks");
            println!("add -> adds new tasks. Usage: todo add <tasks...> [options]");
            println!("delete -> deletes tasks. Usage: todo delete <index_of_tasks...>");
            println!("done -> checks a task. Usage todo done <index_of_tasks...>");
            println!("undone -> unchecks a task. Usage todo undone <index_of_tasks...>");
        }
        _ => println!("Unknown command. use \"todo help\" to show available commands"),
    }
}
