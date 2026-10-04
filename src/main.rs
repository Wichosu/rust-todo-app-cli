use std::env;

mod cli;
mod db;
mod export;
mod filter;
mod todo;
mod ui;
mod undo;

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let conn = db::connect()?;

    match args.get(1).map(|s| s.as_str()) {
        Some("ui") => ui::run(&conn)?,
        _ => cli::run(&conn, &args)?,
    }
    Ok(())
}

fn main() {
    if let Err(err) = run() {
        eprintln!("Error: {}", err);
    }
}
