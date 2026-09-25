use chrono::{DateTime, Local};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Terminal,
};
use std::env;
use std::io;
use rusqlite::Connection;

use crate::todo::Priority;
mod db;
mod todo;

enum AppState {
    Main,
    AddingTask { input: String },
    ConfirmDelete { id: i64, text: String },
    EditTask { id: i64, input: String },
}

fn centered_rect(percent_x: u16, height: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - height) / 2),
            Constraint::Length(height),
            Constraint::Percentage((100 - height) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(ratatui::layout::Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn sort_todos(todos: &mut Vec<crate::todo::Todo>) {
    todos.sort_by(|a, b| {
        let sort_key = |t: &crate::todo::Todo| -> u8 {
            if t.completed {
                return 3;
            }
            match t.priority {
                Priority::High => 0,
                Priority::Medium => 1,
                Priority::Low => 2,
            }
        };
        sort_key(a).cmp(&sort_key(b))
    });
}

fn build_todo_items(
    todos: &[crate::todo::Todo],
    selected: usize,
    highlight_style: Style,
    dimmed: bool,
) -> Vec<ListItem<'_>> {
    todos
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let (label, style) = if t.completed {
                ("[X]".to_string(), Style::default().fg(Color::DarkGray))
            } else if dimmed {
                match t.priority {
                    Priority::High => ("[H]".into(), Style::default().fg(Color::DarkGray)),
                    Priority::Medium => ("[M]".into(), Style::default().fg(Color::DarkGray)),
                    Priority::Low => ("[L]".into(), Style::default().fg(Color::DarkGray)),
                }
            } else {
                match t.priority {
                    Priority::High => ("[H]".into(), Style::default().fg(Color::Red)),
                    Priority::Medium => ("[M]".into(), Style::default().fg(Color::Yellow)),
                    Priority::Low => ("[L]".into(), Style::default().fg(Color::Green)),
                }
            };

            let left_text = format!("{} {}", label, t.text);
            let due_display = t
                .due_date
                .as_ref()
                .map(|d| format!("Due: {}", d))
                .unwrap_or_default();
            let gap = if due_display.is_empty() { "" } else { "  " };
            let due_style = Style::default().fg(Color::DarkGray);

            let item_style = if i == selected {
                highlight_style
            } else {
                style
            };

            let line = Line::from(vec![
                Span::styled(left_text, item_style),
                Span::raw(gap),
                Span::styled(due_display, due_style),
            ]);
            ListItem::new(line)
        })
        .collect()
}

fn run_ui(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    let mut todos = db::list_tasks(conn, None, None, None, None, None, None)?;
    sort_todos(&mut todos);

    let mut selected: usize = 0;
    let mut state = AppState::Main;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    loop {
        let todo_count = todos.len();

        terminal.draw(|f| {
            let area = f.area();

            match &state {
                AppState::Main => {
                    let block = Block::default()
                        .title("Todo List")
                        .borders(Borders::ALL)
                        .style(Style::default().fg(Color::White));

                    let highlight_style = Style::default()
                        .bg(Color::Rgb(40, 40, 60))
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD);

                    let items = build_todo_items(&todos, selected, highlight_style, false);

                    let list = List::new(items)
                        .block(block)
                        .highlight_style(highlight_style);
                    f.render_widget(list, area);

                    let footer_area = Rect {
                        x: area.x + 1,
                        y: area.y + area.height.saturating_sub(2),
                        width: area.width.saturating_sub(2),
                        height: 1,
                    };
                    let footer = Paragraph::new("↑↓ navigate | SPACE toggle | n new | d delete | e edit | q quit")
                        .style(Style::default().fg(Color::DarkGray))
                        .alignment(ratatui::layout::Alignment::Center);
                    f.render_widget(footer, footer_area);
                }
                AppState::AddingTask { input } => {
                    let block = Block::default()
                        .title("Todo List")
                        .borders(Borders::ALL)
                        .style(Style::default().fg(Color::DarkGray));

                    let highlight_style = Style::default()
                        .bg(Color::Rgb(40, 40, 60))
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD);

                    let items = build_todo_items(&todos, selected, highlight_style, true);

                    let list = List::new(items)
                        .block(block)
                        .highlight_style(highlight_style);
                    f.render_widget(list, area);

                    let popup = centered_rect(60, 3, area);
                    f.render_widget(Clear, popup);

                    let popup_block = Block::default()
                        .title("New Task")
                        .borders(Borders::ALL)
                        .style(Style::default().fg(Color::Cyan));
                    let input_display = format!("{}▌", input);
                    let input_paragraph =
                        Paragraph::new(input_display).block(popup_block);
                    f.render_widget(input_paragraph, popup);

                    let footer_area = Rect {
                        x: area.x + 1,
                        y: area.y + area.height.saturating_sub(2),
                        width: area.width.saturating_sub(2),
                        height: 1,
                    };
                    let footer =
                        Paragraph::new("Type your task | ENTER confirm | ESC cancel")
                            .style(Style::default().fg(Color::DarkGray))
                            .alignment(ratatui::layout::Alignment::Center);
                    f.render_widget(footer, footer_area);
                }
                AppState::ConfirmDelete { id: _, text } => {
                    let block = Block::default()
                        .title("Todo List")
                        .borders(Borders::ALL)
                        .style(Style::default().fg(Color::DarkGray));

                    let highlight_style = Style::default()
                        .bg(Color::Rgb(40, 40, 60))
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD);

                    let items = build_todo_items(&todos, selected, highlight_style, true);

                    let list = List::new(items)
                        .block(block)
                        .highlight_style(highlight_style);
                    f.render_widget(list, area);

                    let popup = centered_rect(60, 3, area);
                    f.render_widget(Clear, popup);

                    let popup_block = Block::default()
                        .title("Delete Task")
                        .borders(Borders::ALL)
                        .style(Style::default().fg(Color::Red));
                    let msg = format!("Delete \"{}\"?", text);
                    let confirm_paragraph =
                        Paragraph::new(msg).block(popup_block);
                    f.render_widget(confirm_paragraph, popup);

                    let footer_area = Rect {
                        x: area.x + 1,
                        y: area.y + area.height.saturating_sub(2),
                        width: area.width.saturating_sub(2),
                        height: 1,
                    };
                    let footer =
                        Paragraph::new("y confirm | n/Esc cancel")
                            .style(Style::default().fg(Color::DarkGray))
                            .alignment(ratatui::layout::Alignment::Center);
                    f.render_widget(footer, footer_area);
                }
                AppState::EditTask { id: _, input } => {
                    let block = Block::default()
                        .title("Todo List")
                        .borders(Borders::ALL)
                        .style(Style::default().fg(Color::DarkGray));

                    let highlight_style = Style::default()
                        .bg(Color::Rgb(40, 40, 60))
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD);

                    let items = build_todo_items(&todos, selected, highlight_style, true);

                    let list = List::new(items)
                        .block(block)
                        .highlight_style(highlight_style);
                    f.render_widget(list, area);

                    let popup = centered_rect(60, 3, area);
                    f.render_widget(Clear, popup);

                    let popup_block = Block::default()
                        .title("Edit Task")
                        .borders(Borders::ALL)
                        .style(Style::default().fg(Color::Yellow));
                    let input_display = format!("{}▌", input);
                    let input_paragraph =
                        Paragraph::new(input_display).block(popup_block);
                    f.render_widget(input_paragraph, popup);

                    let footer_area = Rect {
                        x: area.x + 1,
                        y: area.y + area.height.saturating_sub(2),
                        width: area.width.saturating_sub(2),
                        height: 1,
                    };
                    let footer =
                        Paragraph::new("ENTER confirm | ESC cancel")
                            .style(Style::default().fg(Color::DarkGray))
                            .alignment(ratatui::layout::Alignment::Center);
                    f.render_widget(footer, footer_area);
                }
            }
        })?;

        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                match &mut state {
                    AppState::Main => match key.code {
                        KeyCode::Char('q') => break,
                        KeyCode::Up => {
                            if todo_count > 0 && selected > 0 {
                                selected -= 1;
                            }
                        }
                        KeyCode::Down => {
                            if todo_count > 0 && selected < todo_count - 1 {
                                selected += 1;
                            }
                        }
                        KeyCode::Char(' ') => {
                            if todo_count > 0 {
                                let todo = &todos[selected];
                                let id = todo.id;
                                if todo.completed {
                                    db::mark_incomplete(conn, &id)?;
                                } else {
                                    db::mark_completed(conn, &id)?;
                                }
                                todos = db::list_tasks(
                                    conn, None, None, None, None, None, None,
                                )?;
                                sort_todos(&mut todos);
                                if selected >= todos.len() {
                                    selected = todos.len().saturating_sub(1);
                                }
                            }
                        }
                        KeyCode::Char('n') => {
                            state = AppState::AddingTask {
                                input: String::new(),
                            };
                        }
                        KeyCode::Char('d') => {
                            if todo_count > 0 {
                                let todo = &todos[selected];
                                state = AppState::ConfirmDelete {
                                    id: todo.id,
                                    text: todo.text.clone(),
                                };
                            }
                        }
                        KeyCode::Char('e') => {
                            if todo_count > 0 {
                                let todo = &todos[selected];
                                state = AppState::EditTask {
                                    id: todo.id,
                                    input: todo.text.clone(),
                                };
                            }
                        }
                        _ => {}
                    },
                    AppState::AddingTask { input } => match key.code {
                        KeyCode::Esc => {
                            state = AppState::Main;
                        }
                        KeyCode::Enter => {
                            if !input.is_empty() {
                                db::add_task(conn, input, Priority::Medium, None)?;
                                todos = db::list_tasks(
                                    conn, None, None, None, None, None, None,
                                )?;
                                sort_todos(&mut todos);
                                selected = todos.len().saturating_sub(1);
                            }
                            state = AppState::Main;
                        }
                        KeyCode::Char(c) => {
                            input.push(c);
                        }
                        KeyCode::Backspace => {
                            input.pop();
                        }
                        _ => {}
                    },
                    AppState::ConfirmDelete { id, text: _ } => match key.code {
                        KeyCode::Char('y') => {
                            db::delete_task(conn, id)?;
                            todos = db::list_tasks(
                                conn, None, None, None, None, None, None,
                            )?;
                            sort_todos(&mut todos);
                            if selected >= todos.len() {
                                selected = todos.len().saturating_sub(1);
                            }
                            state = AppState::Main;
                        }
                        KeyCode::Char('n') | KeyCode::Esc => {
                            state = AppState::Main;
                        }
                        _ => {}
                    },
                    AppState::EditTask { id, input } => match key.code {
                        KeyCode::Esc => {
                            state = AppState::Main;
                        }
                        KeyCode::Enter => {
                            if !input.is_empty() {
                                db::update_task_text(conn, id, input)?;
                                todos = db::list_tasks(
                                    conn, None, None, None, None, None, None,
                                )?;
                                sort_todos(&mut todos);
                            }
                            state = AppState::Main;
                        }
                        KeyCode::Char(c) => {
                            input.push(c);
                        }
                        KeyCode::Backspace => {
                            input.pop();
                        }
                        _ => {}
                    },
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

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
            println!("search -> searches tasks by text. Usage: todo search <query> [--pending|--completed] [--priority <level>] [--due-today] [--overdue]");
            println!("ui -> launches the terminal UI");
        }
        "search" => {
            if args.len() < 3 {
                println!("Usage: todo search <query> [--pending|--completed] [--priority <level>] [--due-today] [--overdue]");
                return Ok(());
            }

            let query = &args[2];

            let completed = match args.iter().position(|arg| arg == "--pending") {
                Some(_) => Some(false),
                None => match args.iter().position(|arg| arg == "--completed") {
                    Some(_) => Some(true),
                    None => None,
                },
            };

            let priority = args
                .iter()
                .position(|arg| arg == "--priority")
                .and_then(|i| args.get(i + 1))
                .map(|v| v.parse::<Priority>())
                .transpose()?;

            let due_today = args.iter().any(|arg| arg == "--due-today");
            let overdue = args.iter().any(|arg| arg == "--overdue");

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

            let todos = db::search_tasks(&conn, query, completed, priority, due_date.as_deref(), overdue_before.as_deref())?;

            if todos.is_empty() {
                println!("No tasks found matching \"{}\"", query);
            } else {
                println!("Search results for \"{}\":", query);
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
        }
        "ui" => {
            run_ui(&conn)?;
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
