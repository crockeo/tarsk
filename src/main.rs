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

mod controller;
mod database;
mod logging;
mod serialization;

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

    pub fn get_heads(&mut self) -> Vec<automerge::ChangeHash> {
        self.doc.get_heads()
    }

    pub fn get_changes(
        &mut self,
        heads: &[automerge::ChangeHash],
    ) -> anyhow::Result<Vec<automerge::Change>> {
        Ok(self
            .doc
            .get_changes(heads)?
            .into_iter()
            .map(automerge::Change::clone)
            .collect())
    }

    pub fn apply_changes<T: IntoIterator<Item = automerge::Change>>(
        &mut self,
        changes: T,
    ) -> anyhow::Result<()> {
        self.doc.apply_changes(changes)?;
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

    pub fn delete_text(&mut self, delete_pos: usize, amount: usize) -> anyhow::Result<()> {
        let (_, id) = self
            .doc
            .get(automerge::ROOT, "text")?
            .ok_or(anyhow!("missing object"))?;

        self.doc.splice(id, delete_pos, amount, [])?;
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
                    if let Err(e) = sync_server.serve_changes() {
                        teprintln!("PULL error: {}", e);
                    }
                }
            });
        }

        {
            let sync_server = sync_server.clone();
            thread::spawn(move || {
                // TODO: make this killable?
                loop {
                    if let Err(e) = sync_server.pull() {
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
                    thread::sleep(Duration::from_millis(1000));
                }
            });
        }

        {
            let sync_server = sync_server.clone();
            thread::spawn(move || {
                // TODO: make this killable?
                loop {
                    let evt = crossterm::event::read();
                    if let Err(_) = evt {
                        continue;
                    }
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

    pub fn delete_text(&self, delete_pos: usize, amount: usize) -> anyhow::Result<()> {
        let mut doc = self.text.lock().unwrap();
        doc.delete_text(delete_pos, amount)
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

    fn serve_changes(self: &Arc<Self>) -> anyhow::Result<()> {
        let (mut stream, _) = self.listener.accept()?;

        // we receive from a client all of their latest changes (their heads)
        let mut buf: [u8; 1024] = [0; 1024];
        let bytes_read = stream.read(&mut buf)?;
        let heads = serialization::deserialize_change_hashes(&buf[0..bytes_read])?;

        // we give them back our set of changes after those heads
        let changes: Vec<automerge::Change> = {
            let mut doc = self.text.lock().unwrap();
            doc.get_changes(&heads[1..])?
        };
        stream.write_all(&serialization::serialize_changes(&changes)?)?;

        Ok(())
    }

    fn pull(self: &Arc<Self>) -> anyhow::Result<()> {
        let mut stream = TcpStream::connect(self.peer)?;

        let heads = {
            let mut doc = self.text.lock().unwrap();
            doc.get_heads()
        };
        let serialized_heads = serialization::serialize_change_hashes(&heads);
        stream.write_all(&serialized_heads)?;

        let mut raw_changes = Vec::new();
        stream.read_to_end(&mut raw_changes)?;
        let changes = serialization::deserialize_changes(&raw_changes)?;
        {
            let mut doc = self.text.lock().unwrap();
            doc.apply_changes(changes)?;
        }

        self.push_event(Event::Pull);
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

    let mut buf: Vec<char> = Vec::new();
    loop {
        let text = sync_server.get_text()?;
        let buf_text: String = buf.clone().into_iter().collect();

        terminal.draw(|f| {
            let size = f.size();
            let paragraph = Paragraph::new(text.clone() + &buf_text)
                .block(Block::default().title("Contents").borders(Borders::ALL));
            f.render_widget(paragraph, size);
        })?;

        match sync_server.get_event() {
            Event::Pull => {
                sync_server.add_text(text.len(), &buf_text)?;
                buf.clear();
            }
            Event::Terminal(evt) => {
                if let crossterm::event::Event::Key(key) = evt {
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char('c')
                    {
                        break;
                    }

                    match key.code {
                        KeyCode::Char(c) => buf.push(c),
                        KeyCode::Enter => buf.push('\n'),
                        KeyCode::Backspace => {
                            if buf.len() > 0 {
                                buf.pop();
                            } else if text.len() > 0 {
                                sync_server.delete_text(text.len() - 1, 1)?
                            }
                        }
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
