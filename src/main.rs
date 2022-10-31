use std::str::FromStr;
use std::sync::Arc;

use crossterm::event::Event;
use crossterm::event::KeyCode;
use crossterm::event::KeyModifiers;
use crossterm::terminal::disable_raw_mode;
use crossterm::terminal::enable_raw_mode;
use tui::backend::CrosstermBackend;
use tui::layout::Constraint;
use tui::layout::Direction;
use tui::layout::Layout;
use tui::widgets::Block;
use tui::widgets::Borders;
use tui::widgets::Paragraph;
use tui::Terminal;

use crate::database::TaskImage;

mod controller;
mod database;
mod logging;

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

    let task_index: usize = 0;
    loop {
        let tasks: Vec<TaskImage> = db
            .list_tasks()?
            .into_iter()
            .flat_map(|task| task.image())
            .collect();

        let (current_title, current_contents) = if let Some(current_task) = tasks.get(task_index) {
            (current_task.title.as_str(), current_task.body.as_str())
        } else {
            ("No Task", "")
        };

        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(30), Constraint::Percentage(70)].as_ref())
                .split(f.size());

            let task_list = Paragraph::new("hello world").block(
                Block::default()
                    .title(format!("Tasks ({})", tasks.len()))
                    .borders(Borders::ALL),
            );

            let task_body = Paragraph::new(current_contents)
                .block(Block::default().title(current_title).borders(Borders::ALL));

            f.render_widget(task_list, chunks[0]);
            f.render_widget(task_body, chunks[1]);
        })?;

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
