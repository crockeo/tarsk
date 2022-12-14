use std::env;
use std::panic;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use crossterm::event::Event;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
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

fn get_database_path() -> anyhow::Result<PathBuf> {
    let home_dir = PathBuf::from_str(&env::var("HOME")?)?;
    Ok(home_dir.join(".cache").join("tarsk.db"))
}

#[tokio::main()]
async fn main() -> anyhow::Result<()> {
    let db_path = get_database_path()?;
    let db = Arc::new(match database::Database::load(db_path) {
        Ok(db) => db,
        Err(_) => database::Database::new()?,
    });
    let controller = controller::Controller::new(db.clone()).await?;

    // This lets us re-establish normal terminal function when we panic! Nice!
    {
        let handler = panic::take_hook();
        db.save::<&PathBuf>(&get_database_path()?)?;
        panic::set_hook(Box::new(move |panic_info| {
            let _ = disable_raw_mode();
            handler(panic_info)
        }));
    }

    print!("{}[2J", 27 as char);

    enable_raw_mode()?;
    let stdout = std::io::stdout();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut state = State::new();
    loop {
        let tasks: Vec<TaskImage> = db
            .list_tasks()?
            .into_iter()
            .flat_map(|task| task.image())
            .collect();

        let task_titles = tasks
            .iter()
            .enumerate()
            .map(|(i, task)| {
                let mut title = task.title.as_str();
                if title.is_empty() {
                    title = "(No Title)";
                }

                if i == state.current_task {
                    format!("> {}", title)
                } else {
                    format!("  {}", title)
                }
            })
            .collect::<Vec<String>>()
            .join("\n");

        let (current_title, current_contents) =
            if let Some(current_task) = tasks.get(state.current_task) {
                (current_task.title.as_str(), current_task.body.as_str())
            } else {
                ("No Task", "")
            };

        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(30), Constraint::Percentage(70)].as_ref())
                .split(f.size());

            let right_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(3), Constraint::Percentage(100)].as_ref())
                .split(chunks[1]);

            let task_list_chunk = chunks[0];
            let title_chunk = right_chunks[0];
            let body_chunk = right_chunks[1];

            let task_list = Paragraph::new(task_titles).block(
                Block::default()
                    .title(format!(
                        "{}Tasks ({})",
                        if state.mode == EditMode::List {
                            "* "
                        } else {
                            ""
                        },
                        tasks.len()
                    ))
                    .borders(Borders::ALL),
            );

            let task_title = Paragraph::new(current_title).block(
                Block::default()
                    .title(format!(
                        "{}Title",
                        if state.mode == EditMode::Title {
                            "* "
                        } else {
                            ""
                        }
                    ))
                    .borders(Borders::ALL),
            );

            let task_body = Paragraph::new(current_contents).block(
                Block::default()
                    .title(format!(
                        "{}Body",
                        if state.mode == EditMode::Body {
                            "* "
                        } else {
                            ""
                        }
                    ))
                    .borders(Borders::ALL),
            );

            f.render_widget(task_list, task_list_chunk);
            f.render_widget(task_title, title_chunk);
            f.render_widget(task_body, body_chunk);
        })?;

        let event = controller.get_event().await;
        if let controller::Event::Terminal(Event::Key(key)) = event {
            if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                break;
            }
        }
        state = state.handle_event(&db, event)?;
    }

    disable_raw_mode()?;
    db.save::<&PathBuf>(&get_database_path()?)?;

    Ok(())
}

struct State {
    current_task: usize,
    mode: EditMode,
}

impl State {
    fn new() -> Self {
        Self {
            current_task: 0,
            mode: EditMode::List,
        }
    }

    fn handle_event(
        mut self,
        db: &database::Database,
        event: controller::Event,
    ) -> anyhow::Result<Self> {
        if let controller::Event::Terminal(Event::Key(key)) = event {
            if key.code == KeyCode::BackTab {
                self.mode = self.mode.prev();
            } else if key.code == KeyCode::Tab {
                self.mode = self.mode.next();
            }

            // TODO: i hate that this has to have a heap allocation every call :(
            let handler = self.mode.handler();
            handler(&mut self, db, key)?;
        }

        Ok(self)
    }
}

#[derive(Eq, PartialEq)]
enum EditMode {
    List,
    Title,
    Body,
}

type Handler = dyn Fn(&mut State, &database::Database, KeyEvent) -> anyhow::Result<()>;

impl EditMode {
    fn next(&self) -> EditMode {
        use EditMode::*;
        match self {
            List => Title,
            Title => Body,
            Body => List,
        }
    }

    fn prev(&self) -> EditMode {
        use EditMode::*;
        match self {
            List => Body,
            Title => List,
            Body => Title,
        }
    }

    fn handler(&self) -> Box<Handler> {
        use EditMode::*;
        Box::new(match self {
            List => EditMode::handle_event_list,
            Title => EditMode::handle_event_title,
            Body => EditMode::handle_event_body,
        })
    }

    fn handle_event_list(
        state: &mut State,
        db: &database::Database,
        event: KeyEvent,
    ) -> anyhow::Result<()> {
        let tasks = db.list_tasks()?;

        match event.code {
            KeyCode::Up => {
                if state.current_task != 0 {
                    state.current_task -= 1;
                }
            }
            KeyCode::Down => {
                if state.current_task < tasks.len() - 1 {
                    state.current_task += 1;
                }
            }
            KeyCode::Char('a') => {
                db.add_task()?;
            }
            _ => {}
        }

        Ok(())
    }

    fn handle_event_title(
        state: &mut State,
        db: &database::Database,
        event: KeyEvent,
    ) -> anyhow::Result<()> {
        let tasks = db.list_tasks()?;
        if state.current_task >= tasks.len() {
            return Ok(());
        }
        let current_task = &tasks[state.current_task];
        let current_task_title = current_task.title()?;

        match event.code {
            KeyCode::Char(c) => {
                current_task.splice_title(current_task_title.len(), 0, c.to_string())?;
            }
            KeyCode::Backspace => {
                current_task.splice_title(current_task_title.len() - 1, 1, "")?;
            }
            _ => {}
        }

        Ok(())
    }

    fn handle_event_body(
        state: &mut State,
        db: &database::Database,
        event: KeyEvent,
    ) -> anyhow::Result<()> {
        let tasks = db.list_tasks()?;
        if state.current_task >= tasks.len() {
            return Ok(());
        }
        let current_task = &tasks[state.current_task];
        let current_task_body = current_task.body()?;

        match event.code {
            KeyCode::Char(c) => {
                current_task.splice_body(current_task_body.len(), 0, c.to_string())?;
            }
            KeyCode::Backspace => {
                current_task.splice_body(current_task_body.len() - 1, 1, "")?;
            }
            _ => {}
        }

        Ok(())
    }
}
