use std::collections::VecDeque;
use std::io::Read;
use std::io::Write;
use std::net::SocketAddr;
use std::net::TcpListener;
use std::net::TcpStream;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::Condvar;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use anyhow::anyhow;
use automerge::transaction::Transactable;
use crossterm::event::KeyCode;
use crossterm::event::KeyModifiers;
use crossterm::terminal::disable_raw_mode;
use crossterm::terminal::enable_raw_mode;
use lazy_static::lazy_static;
use tui::backend::CrosstermBackend;
use tui::widgets::Block;
use tui::widgets::Borders;
use tui::widgets::Paragraph;
use tui::Terminal;

mod event_stream;

// logging! :)
lazy_static! {
    static ref PRINT_LOCK: Mutex<()> = Mutex::new(());
}

macro_rules! teprintln {
    () => {
        $crate::eprint!("\n")
    };
    ($($arg:tt)*) => {{
	let _ = PRINT_LOCK.lock().unwrap();
	eprintln!($($arg)*);
    }};
}

// 1. make an automerge document that has some kind of content in it
//    see https://github.com/automerge/automerge-rs
pub struct AutomergeText {
    doc: automerge::AutoCommit,
}

impl AutomergeText {
    pub fn new() -> anyhow::Result<Self> {
        let mut doc = automerge::AutoCommit::new();
        doc.put_object(&automerge::ROOT, "text", automerge::ObjType::List)?;
        doc.set_actor(automerge::ActorId::random());
        Ok(Self { doc })
    }

    pub fn load(data: &[u8]) -> anyhow::Result<Self> {
        Ok(Self {
            doc: automerge::AutoCommit::load(data)?,
        })
    }

    pub fn save(&mut self) -> Vec<u8> {
        self.doc.save()
    }

    pub fn merge(&mut self, other: &mut AutomergeText) -> anyhow::Result<()> {
        self.doc.merge(&mut other.doc)?;
        Ok(())
    }

    pub fn add_text<S: AsRef<str>>(
        &mut self,
        insert_pos: usize,
        contents: S,
    ) -> anyhow::Result<()> {
        let (_, id) = self
            .doc
            .get(automerge::ROOT, "text")?
            .ok_or(anyhow!("missing object"))?;

        self.doc.splice_text(id, insert_pos, 0, contents.as_ref())?;

        Ok(())
    }

    pub fn get_text(&mut self) -> anyhow::Result<String> {
        let (_, id) = self
            .doc
            .get(automerge::ROOT, "text")?
            .ok_or(anyhow!("missing object"))?;

        Ok(self.doc.text(id)?)
    }
}

pub enum Event {
    Pull,
    Terminal(crossterm::event::Event),
}

// 2. make a peering server which periodically syncs up the content
//    shared inside of the automerge implementation
pub struct SyncServer {
    listener: TcpListener,
    peer: SocketAddr,
    text: Mutex<AutomergeText>,

    event_queue: Mutex<VecDeque<Event>>,
    has_event: Condvar,
}

impl SyncServer {
    pub fn new(our_port: u16, their_port: u16) -> anyhow::Result<Arc<Self>> {
        let listener = TcpListener::bind(format!("127.0.0.1:{}", our_port))?;
        let peer = SocketAddr::from_str(&format!("127.0.0.1:{}", their_port))?;
        let sync_server = Arc::new(SyncServer {
            listener,
            peer,
            text: Mutex::new(AutomergeText::new()?),
            event_queue: Mutex::new(VecDeque::new()),
            has_event: Condvar::new(),
        });

        {
            let sync_server = sync_server.clone();
            thread::spawn(move || {
                // TODO: make this killable?
                loop {
                    if let Err(e) = sync_server.pull() {
                        teprintln!("PULL error: {}", e);
                    }
                    thread::sleep(Duration::from_millis(100));
                }
            });
        }

        {
            let sync_server = sync_server.clone();
            thread::spawn(move || {
                // TODO: make this killable?
                loop {
                    if let Err(e) = sync_server.push() {
                        // TODO: this is so ugly...
                        match e.downcast_ref::<std::io::Error>() {
                            Some(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
                                continue;
                            }
                            Some(_) => {}
                            None => {}
                        }
                        teprintln!("PUSH error: {}", e);
                    }
                    thread::sleep(Duration::from_millis(100));
                }
            });
        }

	{
	    let sync_server = sync_server.clone();
	    thread::spawn(move || {
                // TODO: make this killable?
                loop {
		    let evt = crossterm::event::read();
		    if let Err(_) = evt { continue; }
		    let evt = evt.unwrap();

		    sync_server.push_event(Event::Terminal(evt));
                }
	    });
	}

        Ok(sync_server)
    }

    pub fn add_text<S: AsRef<str>>(&self, insert_pos: usize, text: S) -> anyhow::Result<()> {
        let mut doc = self.text.lock().unwrap();
        doc.add_text(insert_pos, text.as_ref())
    }

    pub fn get_text(&self) -> anyhow::Result<String> {
        let mut doc = self.text.lock().unwrap();
        doc.get_text()
    }

    pub fn get_event(&self) -> Event {
        let mut event_queue_guard = self.event_queue.lock().unwrap();
        while event_queue_guard.len() == 0 {
            event_queue_guard = self.has_event.wait(event_queue_guard).unwrap();
        }
        event_queue_guard.pop_front().unwrap()
    }

    fn push_event(&self, evt: Event) {
	let mut event_queue_guard = self.event_queue.lock().unwrap();
	event_queue_guard.push_back(evt);
	self.has_event.notify_one();

    }

    fn pull(self: &Arc<Self>) -> anyhow::Result<()> {
        let (mut stream, _) = self.listener.accept()?;

        let mut contents = vec![];
        stream.read_to_end(&mut contents)?;

        let mut other_doc = AutomergeText::load(&contents)?;
        let mut doc = self.text.lock().unwrap();
        doc.merge(&mut other_doc)?;

	self.push_event(Event::Pull);

        Ok(())
    }

    fn push(self: &Arc<Self>) -> anyhow::Result<()> {
        let mut stream = TcpStream::connect(self.peer)?;
        let buf = {
            let mut doc = self.text.lock().unwrap();
            doc.save()
        };
        stream.write_all(&buf)?;
        Ok(())
    }
}

// 3. make a user interface that lets people modify the content
//    inside of the automerge document
fn main() -> anyhow::Result<()> {
    print!("{}[2J", 27 as char);

    let args: Vec<String> = std::env::args().into_iter().collect();
    let our_port = u16::from_str(&args[1])?;
    let their_port = u16::from_str(&args[2])?;

    let sync_server = SyncServer::new(our_port, their_port)?;

    enable_raw_mode()?;
    let stdout = std::io::stdout();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // thinking:
    //
    // i need to make some kind of unified event pipeline
    // pretty much i want to:
    // - be able to handle user events
    // - refresh whenever i see that the server has synced
    // - and then ??? there was a third thing but i forget

    loop {
        let text = sync_server.get_text()?;

        terminal.draw(|f| {
            let size = f.size();
            let paragraph = Paragraph::new(text.clone())
                .block(Block::default().title("Contents").borders(Borders::ALL));
            f.render_widget(paragraph, size);
        })?;

        match sync_server.get_event() {
            Event::Pull => {},
            Event::Terminal(evt) => {
                if let crossterm::event::Event::Key(key) = evt {
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char('c')
                    {
                        break;
                    }

                    match key.code {
                        KeyCode::Char(c) => sync_server.add_text(text.len(), c.to_string())?,
                        _ => {}
                    }
                }
            }
        }
    }

    disable_raw_mode()?;

    Ok(())
}

#[cfg(test)]
mod automerge_number_tests {
    use super::*;

    #[test]
    fn test_empty_text() -> anyhow::Result<()> {
        let mut doc = AutomergeText::new()?;
        assert_eq!(doc.get_text()?, "");
        Ok(())
    }

    #[test]
    fn test_add_text() -> anyhow::Result<()> {
        let mut doc = AutomergeText::new()?;
        doc.add_text(0, "hello world!")?;
        assert_eq!(doc.get_text()?, "hello world!");
        Ok(())
    }

    #[test]
    fn test_add_interleaved_text() -> anyhow::Result<()> {
        let mut doc = AutomergeText::new()?;
        doc.add_text(0, "world!")?;
        doc.add_text(0, "hello ")?;
        assert_eq!(doc.get_text()?, "hello world!");
        Ok(())
    }
}
