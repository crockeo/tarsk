use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::sync::Mutex;

use automerge::Change;
use automerge::ChangeHash;
use automerge::ExpandedChange;

use crate::database::Database;
use crate::logging;

pub struct Controller {
    database: Arc<Database>,

    listener: TcpListener,
    peer: SocketAddr,

    tx: mpsc::UnboundedSender<Event>,
    rx: Mutex<mpsc::UnboundedReceiver<Event>>,
}

impl Controller {
    pub async fn new(
        database: Arc<Database>,
        our_port: u16,
        their_port: u16,
    ) -> anyhow::Result<Arc<Self>> {
        let listener = TcpListener::bind(format!("127.0.0.1:{}", our_port)).await?;
        let peer = SocketAddr::from_str(&format!("127.0.0.1:{}", their_port))?;

        let (tx, rx) = mpsc::unbounded_channel();
        let server = Arc::new(Self {
            database,
            listener,
            peer,
            tx,
            rx: Mutex::new(rx),
        });

        {
            let server = server.clone();
            tokio::spawn(server.serve_thread());
        }

        {
            let server = server.clone();
            tokio::spawn(server.pull_thread());
        }

        {
            // This is handled on its own operating system thread
            // because waiting for terminal input is not async.
            // If it were a normal async task it could freeze the event hub.
            let server = server.clone();
            thread::spawn(|| server.poll_terminal_thread());
        }

        Ok(server)
    }

    pub async fn get_event(self: &Arc<Self>) -> Event {
        let mut rx = self.rx.lock().await;
        rx.recv().await.expect("Failed to poll event.")
    }

    async fn serve_thread(self: Arc<Self>) {
        loop {
            if let Err(e) = self.serve().await {
                logging::GLOBAL.error(&format!("Error while serving: {}", e));
            }
            thread::sleep(Duration::from_millis(1000));
        }
    }

    async fn serve(self: &Arc<Self>) -> anyhow::Result<()> {
        let (stream, _) = self.listener.accept().await?;
        let (read_half, mut write_half) = stream.into_split();

        // we receive from a client all of their latest changes (their heads)
        let mut reader = BufReader::new(read_half);
        let mut raw_heads = Vec::new();
        reader.read_until(b'\n', &mut raw_heads).await?;
        let heads = deserialize_change_hashes(&raw_heads)?;

        // we give them back our set of changes after those heads
        let changes = self.database.get_changes(&heads)?;
        let serialized_changes = serialize_changes(&changes)?;
        write_half.write_all(&serialized_changes).await?;

        Ok(())
    }

    async fn pull_thread(self: Arc<Self>) {
        loop {
            if let Err(e) = self.pull().await {
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

    async fn pull(self: &Arc<Self>) -> anyhow::Result<()> {
        let stream = TcpStream::connect(self.peer).await?;
        let (read_half, mut write_half) = stream.into_split();

        let heads = self.database.get_heads();
        let raw_heads = serialize_change_hashes(&heads[1..])?;
        write_half.write_all(&raw_heads).await?;
        write_half.write_all(b"\n").await?;

        let mut reader = BufReader::new(read_half);
        let mut raw_changes = Vec::new();
        reader.read_until(b'\n', &mut raw_changes).await?;
        let changes = deserialize_changes(&raw_changes)?;

        self.database.apply_changes(changes)?;

        self.tx.send(Event::Pull)?;

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

        self.tx.send(Event::Terminal(evt))?;
        Ok(())
    }
}

#[derive(Debug)]
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
        let _ = doc.put(automerge::ROOT, "number", 1234);

        let changes: Vec<automerge::Change> = doc
            .get_changes(&[])
            .unwrap()
            .into_iter()
            .map(automerge::Change::clone)
            .collect();

        let raw = serialize_changes(&changes).unwrap();
        let deserialized_changes = deserialize_changes(&raw);
        assert!(deserialized_changes.is_ok());
        assert_eq!(changes, deserialized_changes.unwrap());
    }
}
