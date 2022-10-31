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

use crate::database::Database;
use crate::logging;
use crate::serialization;

pub struct Controller {
    database: Arc<Database>,

    listener: TcpListener,
    peer: SocketAddr,

    event_queue: Mutex<VecDeque<Event>>,
    has_event: Condvar,
}

impl Controller {
    pub fn new(
        database: Arc<Database>,
        our_port: u16,
        their_port: u16,
    ) -> anyhow::Result<Arc<Self>> {
        let listener = TcpListener::bind(format!("127.0.0.1:{}", our_port))?;
        let peer = SocketAddr::from_str(&format!("127.0.0.1:{}", their_port))?;
        let server = Arc::new(Self {
            database,
            listener,
            peer,
            event_queue: Mutex::new(VecDeque::new()),
            has_event: Condvar::new(),
        });

        {
            let server = server.clone();
            thread::spawn(move || server.serve_thread());
        }

        {
            let server = server.clone();
            thread::spawn(move || server.pull_thread());
        }

        {
            let server = server.clone();
            thread::spawn(move || server.poll_terminal_thread());
        }

        Ok(server)
    }

    pub fn get_event(self: &Arc<Self>) -> Event {
        let mut event_queue = self.event_queue.lock().unwrap();
        while event_queue.len() == 0 {
            event_queue = self.has_event.wait(event_queue).unwrap();
        }
        event_queue.pop_front().unwrap()
    }

    fn serve_thread(self: Arc<Self>) {
        loop {
            if let Err(e) = self.serve() {
                let _ = logging::GLOBAL.log(&format!("Error while serving: {}", e));
            }
        }
    }

    fn serve(self: &Arc<Self>) -> anyhow::Result<()> {
        let (mut stream, _) = self.listener.accept()?;

        // we receive from a client all of their latest changes (their heads)
        let mut buf: [u8; 1024] = [0; 1024];
        let bytes_read = stream.read(&mut buf)?;
        let heads = serialization::deserialize_change_hashes(&buf[0..bytes_read])?;

        // we give them back our set of changes after those heads
        let changes = self.database.get_changes(&heads)?;
        stream.write_all(&serialization::serialize_changes(&changes)?)?;

        Ok(())
    }

    fn pull_thread(self: Arc<Self>) {
        loop {
            if let Err(e) = self.pull() {
		// TODO: make this less ugly whenever this feature becomes stable?
                if let Some(e) = e.downcast_ref::<std::io::Error>() {
		    if e.kind() == std::io::ErrorKind::ConnectionRefused {
			continue
		    }
		}

                let _ = logging::GLOBAL.log(&format!("Error while pulling: {}", e));
            }
        }
    }

    fn pull(self: &Arc<Self>) -> anyhow::Result<()> {
        let mut stream = TcpStream::connect(self.peer)?;

        let timeout = Some(Duration::from_secs(1));
        stream.set_read_timeout(timeout)?;
        stream.set_write_timeout(timeout)?;

        let heads = &self.database.get_heads()[1..];
        let serialized_heads = serialization::serialize_change_hashes(&heads);
        stream.write_all(&serialized_heads)?;

        let mut raw_changes = Vec::new();
        stream.read_to_end(&mut raw_changes)?;
        let changes = serialization::deserialize_changes(&raw_changes)?;
        self.database.apply_changes(changes)?;

        let mut event_queue = self.event_queue.lock().unwrap();
        event_queue.push_back(Event::Pull);
        self.has_event.notify_one();

        let _ = logging::GLOBAL.log("Successfully pulled!");

        Ok(())
    }

    fn poll_terminal_thread(self: Arc<Self>) {
        loop {
            if let Err(e) = self.poll_terminal() {
                let _ = logging::GLOBAL.log(&format!("Error while polling: {}", e));
            }
        }
    }

    fn poll_terminal(self: &Arc<Self>) -> anyhow::Result<()> {
        let evt = crossterm::event::read()?;
        let mut event_queue = self.event_queue.lock().unwrap();
        event_queue.push_back(Event::Terminal(evt));
        self.has_event.notify_one();
        Ok(())
    }
}

pub enum Event {
    Pull,
    Terminal(crossterm::event::Event),
}
