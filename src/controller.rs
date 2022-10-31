use std::collections::VecDeque;
use std::io::BufRead;
use std::io::BufReader;
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

use automerge::Change;
use automerge::ChangeHash;
use automerge::ExpandedChange;

use crate::database::Database;
use crate::logging;

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
                logging::GLOBAL.error(&format!("Error while serving: {}", e));
            }
            thread::sleep(Duration::from_millis(1000));
        }
    }

    fn serve(self: &Arc<Self>) -> anyhow::Result<()> {
        let (mut stream, _) = self.listener.accept()?;
        let mut reader = BufReader::new(stream.try_clone()?);

        // we receive from a client all of their latest changes (their heads)
        let mut raw_heads = Vec::new();
        reader.read_until(b'\n', &mut raw_heads)?;
        let heads = deserialize_change_hashes(&raw_heads)?;

        // we give them back our set of changes after those heads
        let changes = self.database.get_changes(&heads)?;
        let serialized_changes = serialize_changes(&changes)?;
        stream.write_all(&serialized_changes)?;

        Ok(())
    }

    fn pull_thread(self: Arc<Self>) {
        loop {
            if let Err(e) = self.pull() {
                // TODO: make this less ugly whenever this feature becomes stable?
                if let Some(e) = e.downcast_ref::<std::io::Error>() {
                    if e.kind() == std::io::ErrorKind::ConnectionRefused {
                        continue;
                    }
                }

                logging::GLOBAL.error(&format!("Error while pulling: {}", e));
            }
            thread::sleep(Duration::from_millis(1000));
        }
    }

    fn pull(self: &Arc<Self>) -> anyhow::Result<()> {
        let mut stream = TcpStream::connect(self.peer)?;
        let mut reader = BufReader::new(stream.try_clone()?);

        let heads = self.database.get_heads();
        let raw_heads = serialize_change_hashes(&heads[1..])?;
        stream.write_all(&raw_heads)?;
        stream.write_all(b"\n")?;

        let mut raw_changes = Vec::new();
        reader.read_until(b'\n', &mut raw_changes)?;
        let changes = deserialize_changes(&raw_changes)?;

        self.database.apply_changes(changes)?;

        let mut event_queue = self.event_queue.lock().unwrap();
        event_queue.push_back(Event::Pull);
        self.has_event.notify_one();

        Ok(())
    }

    fn poll_terminal_thread(self: Arc<Self>) {
        loop {
            if let Err(e) = self.poll_terminal() {
                logging::GLOBAL.error(&format!("Error while polling: {}", e));
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

pub fn serialize_change_hashes(hashes: &[ChangeHash]) -> anyhow::Result<Vec<u8>> {
    let serialized = serde_json::to_string(hashes)?;
    Ok(serialized.into_bytes())
}

pub fn deserialize_change_hashes(bytes: &[u8]) -> anyhow::Result<Vec<ChangeHash>> {
    let serialized = std::str::from_utf8(bytes)?;
    let hashes: Vec<ChangeHash> = serde_json::from_str(serialized)?;
    Ok(hashes)
}

pub fn serialize_changes(changes: &[Change]) -> anyhow::Result<Vec<u8>> {
    let serialized = serde_json::to_string(
        &changes
            .iter()
            .map(|change| change.decode())
            .collect::<Vec<ExpandedChange>>(),
    )?;
    Ok(serialized.into_bytes())
}

pub fn deserialize_changes(bytes: &[u8]) -> anyhow::Result<Vec<Change>> {
    let serialized = std::str::from_utf8(bytes)?;
    let changes = serde_json::from_str::<Vec<ExpandedChange>>(serialized)?;
    Ok(changes.into_iter().map(ExpandedChange::into).collect())
}

#[cfg(test)]
mod tests {
    use automerge::transaction::Transactable;
    use automerge::AutoCommit;

    use super::*;

    #[test]
    fn test_change_hashes_roundtrip() {
        let change_hashes = vec![ChangeHash([0; 32])];
        let raw = serialize_change_hashes(&change_hashes).unwrap();
        let deserialized_change_hashes = deserialize_change_hashes(&raw);
        assert!(deserialized_change_hashes.is_ok());
        assert_eq!(change_hashes, deserialized_change_hashes.unwrap());
    }

    #[test]
    fn test_change_roundtrip() {
        let mut doc = AutoCommit::new();
        _ = doc.put(automerge::ROOT, "number", 1234);

        let changes: Vec<automerge::Change> = doc
            .get_changes(&[])
            .unwrap()
            .into_iter()
            .map(automerge::Change::clone)
            .collect();

        let raw = serialize_changes(&changes).unwrap();
        let deserialized_changes = deserialize_changes(&raw);
        assert!(deserialized_changes.is_ok());
        assert_eq!(
            changes
                .into_iter()
                .map(|change| change.clone())
                .collect::<Vec<Change>>(),
            deserialized_changes.unwrap()
        );
    }
}
