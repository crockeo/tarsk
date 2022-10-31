use std::str::FromStr;
use std::sync::Arc;

use crossterm::event::Event;
use crossterm::event::KeyCode;
use crossterm::event::KeyModifiers;
use crossterm::terminal::disable_raw_mode;
use crossterm::terminal::enable_raw_mode;
use tui::backend::CrosstermBackend;
use tui::widgets::Block;
use tui::widgets::Borders;
use tui::widgets::Paragraph;
use tui::Terminal;

use crate::database::TaskImage;

mod controller;
mod database;
mod logging;
mod serialization;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().into_iter().collect();
    let our_port = u16::from_str(&args[1])?;
    let their_port = u16::from_str(&args[2])?;

    let db = Arc::new(database::Database::new()?);
    let controller = controller::Controller::new(db.clone(), our_port, their_port)?;

    print!("{}[2J", 27 as char);

    enable_raw_mode()?;
    let stdout = std::io::stdout();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    loop {
        let tasks: Vec<TaskImage> = db
            .list_tasks()?
            .into_iter()
            .flat_map(|task| task.image())
            .collect();

        terminal.draw(|f| {
            let size = f.size();
            let paragraph = Paragraph::new(tasks.len().to_string())
                .block(Block::default().title("Contents").borders(Borders::ALL));
            f.render_widget(paragraph, size);
        });

        match controller.get_event() {
            controller::Event::Terminal(Event::Key(key)) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                    break;
                }

		if key.code == KeyCode::Char('a') {
		    db.add_task()?;
		}
            }
            _ => {}
        }
    }

    disable_raw_mode()?;

    Ok(())
}
