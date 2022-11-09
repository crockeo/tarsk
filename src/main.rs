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

use crate::database::Task;
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

    // TODO: build this out into a proof-of-concept TUI
    //
    // - render tasks down the left side
    // - have TAB swap between:
    //   - list of tasks on the LHS
    //     - up and down control which is currently selected
    //   - title of current task
    //     - normal editing (full character passthrough, incl delete)
    //   - body of current task
    //     - normal editing (full character passthrough, incl delete)
    // - have shift+TAB go backwards
    //
    // - to consider after:
    //   - how would one make a filtered view (e.g. things i've scheduled today)
    //   - how would one make this more efficient? e.g. debouncing edits to tasks
    //     - maybe look into a version of the db which doesn't autocommit
    //       to have more fine-grained control over this?
    let mut state = State::new();
    loop {
        let tasks: Vec<TaskImage> = db
            .list_tasks()?
            .into_iter()
            .flat_map(|task| task.image())
            .collect();

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

            let task_list = Paragraph::new("hello world").block(
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

        let event = controller.get_event();
        if let controller::Event::Terminal(Event::Key(key)) = event {
            if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                break;
            }
        }
        state = state.handle_event(&db, event)?;
    }

    disable_raw_mode()?;

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

	    self.mode.handle_event(db, key)?;
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

    fn handle_event(&self, db: &database::Database, event: KeyEvent) -> anyhow::Result<()> {
	use EditMode::*;
	match self {
	    List => EditMode::handle_event_list(db, event),
	    Title => EditMode::handle_event_title(db, event),
	    Body => EditMode::handle_event_body(db, event),
	}
    }

    fn handle_event_list(db: &database::Database, event: KeyEvent) -> anyhow::Result<()> {
	Ok(())
    }

    fn handle_event_title(db: &database::Database, event: KeyEvent) -> anyhow::Result<()> {
	Ok(())
    }

    fn handle_event_body(db: &database::Database, event: KeyEvent) -> anyhow::Result<()> {
	Ok(())
    }
}
