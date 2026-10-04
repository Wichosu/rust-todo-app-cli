use chrono::NaiveDate;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame, Terminal,
};
use rusqlite::Connection;
use std::io;

use crate::db;
use crate::filter::{
    DateFilter, StatusFilter, TodoFilter, cycle_index, priority_from_index, priority_index, today,
};
use crate::todo::{Priority, Repeat, Todo};
use crate::undo;

enum AppState {
    Main,
    AddingTask { input: String },
    ConfirmDelete { id: i64, text: String },
    EditTask { id: i64, input: String },
    FilterMenu {
        row: usize,
        status: StatusFilter,
        priority: Option<Priority>,
        date: DateFilter,
        date_value: String,
    },
}

enum EventOutcome {
    Continue,
    Quit,
}

fn highlight_style() -> Style {
    Style::default()
        .bg(Color::Rgb(40, 40, 60))
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}

fn centered_rect(width_percent: u16, height_rows: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - height_rows) / 2),
            Constraint::Length(height_rows),
            Constraint::Percentage((100 - height_rows) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - width_percent) / 2),
            Constraint::Percentage(width_percent),
            Constraint::Percentage((100 - width_percent) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn build_todo_items(
    todos: &[Todo],
    selected: usize,
    highlight: Style,
    dimmed: bool,
) -> Vec<ListItem<'_>> {
    todos
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let label = if t.completed {
                "[X]"
            } else {
                match t.priority {
                    Priority::High => "[H]",
                    Priority::Medium => "[M]",
                    Priority::Low => "[L]",
                }
            };
            let style = if t.completed || dimmed {
                Style::default().fg(Color::DarkGray)
            } else {
                match t.priority {
                    Priority::High => Style::default().fg(Color::Red),
                    Priority::Medium => Style::default().fg(Color::Yellow),
                    Priority::Low => Style::default().fg(Color::Green),
                }
            };

            let left_text = format!("{} {}", label, t.text);
            let due_display = t
                .due_date
                .as_ref()
                .map(|d| format!("Due: {}", d))
                .unwrap_or_default();
            let due_style = Style::default().fg(Color::DarkGray);

            let item_style = if i == selected { highlight } else { style };

            let mut spans = vec![Span::styled(left_text, item_style)];
            if !due_display.is_empty() {
                spans.push(Span::raw("  "));
                spans.push(Span::styled(due_display, due_style));
            }
            if t.repeat != Repeat::None {
                spans.push(Span::raw("  "));
                spans.push(Span::styled(
                    format!("· {}", t.repeat),
                    Style::default().fg(Color::Cyan),
                ));
            }
            ListItem::new(Line::from(spans))
        })
        .collect()
}

fn render_todo_background(
    f: &mut Frame,
    area: Rect,
    todos: &[Todo],
    selected: usize,
    title: String,
    dimmed: bool,
) {
    let block_color = if dimmed { Color::DarkGray } else { Color::White };
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .style(Style::default().fg(block_color));

    let highlight = highlight_style();
    let items = build_todo_items(todos, selected, highlight, dimmed);
    let list = List::new(items)
        .block(block)
        .highlight_style(highlight);
    f.render_widget(list, area);
}

fn render_footer(f: &mut Frame, area: Rect, text: &str) {
    let footer_area = Rect {
        x: area.x + 1,
        y: area.y + area.height.saturating_sub(2),
        width: area.width.saturating_sub(2),
        height: 1,
    };
    let footer = Paragraph::new(text)
        .style(Style::default().fg(Color::DarkGray))
        .alignment(ratatui::layout::Alignment::Center);
    f.render_widget(footer, footer_area);
}

fn render_popup(
    f: &mut Frame,
    area: Rect,
    title: &str,
    color: Color,
    width_percent: u16,
    height_rows: u16,
) -> Rect {
    let popup = centered_rect(width_percent, height_rows, area);
    f.render_widget(Clear, popup);
    let block = Block::default()
        .title(title.to_string())
        .borders(Borders::ALL)
        .style(Style::default().fg(color));
    f.render_widget(block, popup);
    Rect {
        x: popup.x + 1,
        y: popup.y + 1,
        width: popup.width.saturating_sub(2),
        height: popup.height.saturating_sub(2),
    }
}

fn option_row(
    marker: &str,
    label: &str,
    options: &[&'static str],
    selected: usize,
    colors: Option<&[Color]>,
    highlight: Style,
) -> Line<'static> {
    let mut spans = vec![Span::styled(
        format!("{}{}", marker, label),
        Style::default().fg(Color::White),
    )];
    for (i, opt) in options.iter().enumerate() {
        if i == selected {
            spans.push(Span::styled(*opt, highlight));
        } else {
            let color = colors.map(|c| c[i]).unwrap_or(Color::White);
            spans.push(Span::styled(*opt, Style::default().fg(color)));
        }
        if i < options.len() - 1 {
            spans.push(Span::raw("  "));
        }
    }
    Line::from(spans)
}

fn render_filter_popup(
    f: &mut Frame,
    area: Rect,
    row: usize,
    status: StatusFilter,
    priority: Option<Priority>,
    date: DateFilter,
    date_value: &str,
) {
    let highlight = highlight_style();
    let needs_value = date.needs_value();
    let popup_height = if needs_value { 6 } else { 5 };
    let inner = render_popup(f, area, "Filter", Color::Cyan, 75, popup_height);

    let status_row = option_row(
        if row == 0 { "> " } else { "  " },
        "Status:   ",
        &["All", "Pending", "Completed"],
        status.index(),
        None,
        highlight,
    );
    let priority_row = option_row(
        if row == 1 { "> " } else { "  " },
        "Priority: ",
        &["All", "Low", "Medium", "High"],
        priority_index(priority),
        Some(&[
            Color::White,
            Color::Green,
            Color::Yellow,
            Color::Red,
        ]),
        highlight,
    );
    let date_row = option_row(
        if row == 2 { "> " } else { "  " },
        "Date:     ",
        &["All", "Due Today", "Overdue", "Before", "After"],
        date.index(),
        None,
        highlight,
    );

    let mut lines = vec![status_row, priority_row, date_row];
    if needs_value {
        lines.push(Line::from(vec![
            Span::styled("  Value:    ", Style::default().fg(Color::White)),
            Span::styled(format!("{}▌", date_value), highlight),
        ]));
    }
    f.render_widget(Paragraph::new(lines), inner);

    let date_valid = NaiveDate::parse_from_str(date_value, "%Y-%m-%d").is_ok();
    let footer_text = if needs_value && !date_valid {
        "Date must be YYYY-MM-DD | ENTER apply | ESC cancel"
    } else {
        "↑↓ row | ←→ value | ENTER apply | ESC cancel"
    };
    render_footer(f, area, footer_text);
}

fn draw(
    f: &mut Frame,
    state: &AppState,
    todos: &[Todo],
    selected: usize,
    filter: &TodoFilter,
) {
    let area = f.area();

    match state {
        AppState::Main => {
            render_todo_background(
                f,
                area,
                todos,
                selected,
                format!("Todo List ({})", filter.label()),
                false,
            );
            render_footer(
                f,
                area,
                "↑↓ navigate | SPACE toggle | n new | d delete | e edit | f filter | u undo | q quit",
            );
        }
        AppState::AddingTask { input } => {
            render_todo_background(f, area, todos, selected, "Todo List".to_string(), true);
            let inner = render_popup(f, area, "New Task", Color::Cyan, 60, 3);
            f.render_widget(Paragraph::new(format!("{}▌", input)), inner);
            render_footer(f, area, "Type your task | ENTER confirm | ESC cancel");
        }
        AppState::ConfirmDelete { text, .. } => {
            render_todo_background(f, area, todos, selected, "Todo List".to_string(), true);
            let inner = render_popup(f, area, "Delete Task", Color::Red, 60, 3);
            f.render_widget(
                Paragraph::new(format!("Delete \"{}\"?", text)),
                inner,
            );
            render_footer(f, area, "y confirm | n/Esc cancel");
        }
        AppState::EditTask { input, .. } => {
            render_todo_background(f, area, todos, selected, "Todo List".to_string(), true);
            let inner = render_popup(f, area, "Edit Task", Color::Yellow, 60, 3);
            f.render_widget(Paragraph::new(format!("{}▌", input)), inner);
            render_footer(f, area, "ENTER confirm | ESC cancel");
        }
        AppState::FilterMenu {
            row,
            status,
            priority,
            date,
            date_value,
        } => {
            render_todo_background(f, area, todos, selected, "Todo List".to_string(), true);
            render_filter_popup(f, area, *row, *status, *priority, *date, date_value);
        }
    }
}

fn reload(
    conn: &Connection,
    todos: &mut Vec<Todo>,
    filter: &TodoFilter,
    selected: &mut usize,
) -> rusqlite::Result<()> {
    *todos = db::load_todos(conn, filter)?;
    if *selected >= todos.len() {
        *selected = todos.len().saturating_sub(1);
    }
    Ok(())
}

fn handle_key(
    conn: &Connection,
    state: &mut AppState,
    todos: &mut Vec<Todo>,
    selected: &mut usize,
    filter: &mut TodoFilter,
    key: KeyCode,
) -> Result<EventOutcome, Box<dyn std::error::Error>> {
    match state {
        AppState::Main => match key {
            KeyCode::Char('q') => return Ok(EventOutcome::Quit),
            KeyCode::Up if !todos.is_empty() && *selected > 0 => {
                *selected -= 1;
            }
            KeyCode::Down if !todos.is_empty() && *selected < todos.len() - 1 => {
                *selected += 1;
            }
            KeyCode::Char(' ') if !todos.is_empty() => {
                let todo = &todos[*selected];
                let id = todo.id;
                let batch = undo::next_batch(conn)?;
                if todo.completed {
                    db::mark_incomplete(conn, &id, batch)?;
                } else {
                    db::mark_completed(conn, &id, batch)?;
                }
                reload(conn, todos, filter, selected)?;
            }
            KeyCode::Char('n') => {
                *state = AppState::AddingTask {
                    input: String::new(),
                };
            }
            KeyCode::Char('d') if !todos.is_empty() => {
                let todo = &todos[*selected];
                *state = AppState::ConfirmDelete {
                    id: todo.id,
                    text: todo.text.clone(),
                };
            }
            KeyCode::Char('e') if !todos.is_empty() => {
                let todo = &todos[*selected];
                *state = AppState::EditTask {
                    id: todo.id,
                    input: todo.text.clone(),
                };
            }
            KeyCode::Char('u') => {
                undo::undo_last(conn)?;
                reload(conn, todos, filter, selected)?;
            }
            KeyCode::Char('f') => {
                *state = AppState::FilterMenu {
                    row: 0,
                    status: filter.status,
                    priority: filter.priority,
                    date: filter.date,
                    date_value: filter.date_value.clone(),
                };
            }
            _ => {}
        },
        AppState::AddingTask { input } => match key {
            KeyCode::Esc => {
                *state = AppState::Main;
            }
            KeyCode::Enter => {
                if !input.is_empty() {
                    let batch = undo::next_batch(conn)?;
                    db::add_task(conn, input, Priority::Medium, None, Repeat::None, batch)?;
                    reload(conn, todos, filter, selected)?;
                    *selected = todos.len().saturating_sub(1);
                }
                *state = AppState::Main;
            }
            KeyCode::Char(c) => {
                input.push(c);
            }
            KeyCode::Backspace => {
                input.pop();
            }
            _ => {}
        },
        AppState::ConfirmDelete { id, text: _ } => match key {
            KeyCode::Char('y') => {
                let batch = undo::next_batch(conn)?;
                db::delete_task(conn, id, batch)?;
                reload(conn, todos, filter, selected)?;
                *state = AppState::Main;
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                *state = AppState::Main;
            }
            _ => {}
        },
        AppState::EditTask { id, input } => match key {
            KeyCode::Esc => {
                *state = AppState::Main;
            }
            KeyCode::Enter => {
                if !input.is_empty() {
                    let batch = undo::next_batch(conn)?;
                    db::update_task_text(conn, id, input, batch)?;
                    reload(conn, todos, filter, selected)?;
                }
                *state = AppState::Main;
            }
            KeyCode::Char(c) => {
                input.push(c);
            }
            KeyCode::Backspace => {
                input.pop();
            }
            _ => {}
        },
        AppState::FilterMenu {
            row,
            status,
            priority,
            date,
            date_value,
        } => match key {
            KeyCode::Up => {
                *row = row.saturating_sub(1);
            }
            KeyCode::Down if *row < 2 => {
                *row += 1;
            }
            KeyCode::Left | KeyCode::Right => {
                let dir = if key == KeyCode::Right { 1 } else { -1 };
                match *row {
                    0 => {
                        let idx = cycle_index(status.index(), dir, 3);
                        *status = StatusFilter::from_index(idx);
                    }
                    1 => {
                        let idx = cycle_index(priority_index(*priority), dir, 4);
                        *priority = priority_from_index(idx);
                    }
                    _ => {
                        let idx = cycle_index(date.index(), dir, 5);
                        *date = DateFilter::from_index(idx);
                        if date.needs_value() && date_value.is_empty() {
                            *date_value = today();
                        }
                    }
                }
            }
            KeyCode::Char(c)
                if date.needs_value()
                    && (c.is_ascii_digit() || c == '-')
                    && date_value.len() < 10 =>
            {
                date_value.push(c);
            }
            KeyCode::Backspace if date.needs_value() => {
                date_value.pop();
            }
            KeyCode::Enter => {
                let date_ok = !date.needs_value()
                    || NaiveDate::parse_from_str(date_value, "%Y-%m-%d").is_ok();
                if date_ok {
                    *filter = TodoFilter {
                        status: *status,
                        priority: *priority,
                        date: *date,
                        date_value: date_value.clone(),
                    };
                    *todos = db::load_todos(conn, filter)?;
                    *selected = 0;
                    *state = AppState::Main;
                }
            }
            KeyCode::Esc => {
                *state = AppState::Main;
            }
            _ => {}
        },
    }
    Ok(EventOutcome::Continue)
}

pub fn run(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    let mut filter = TodoFilter {
        status: StatusFilter::All,
        priority: None,
        date: DateFilter::All,
        date_value: String::new(),
    };
    let mut todos = db::load_todos(conn, &filter)?;
    let mut selected: usize = 0;
    let mut state = AppState::Main;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    loop {
        terminal.draw(|f| draw(f, &state, &todos, selected, &filter))?;

        if let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            let outcome = handle_key(conn, &mut state, &mut todos, &mut selected, &mut filter, key.code)?;
            if matches!(outcome, EventOutcome::Quit) {
                break;
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}
