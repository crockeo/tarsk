use std::net::Ipv4Addr;
use std::net::SocketAddr;
use std::net::SocketAddrV4;
use std::sync::Arc;
use std::thread;

use automerge::Change;
use automerge::ChangeHash;
use automerge::ExpandedChange;
use lazy_static::lazy_static;
use tokio::sync::mpsc;
use tokio::sync::Mutex;

use self::registry::Registry;
use self::sync::Sync;
use crate::database::Database;
use crate::logging;

mod registry;
mod sync;
mod utils;

lazy_static! {
    static ref REGISTRY_ADDR: SocketAddr =
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 8084));
}

pub struct Controller {
    database: Arc<Database>,

    registry: Arc<Registry>,
    sync: Arc<Sync>,

    tx: mpsc::UnboundedSender<Event>,
    rx: Mutex<mpsc::UnboundedReceiver<Event>>,
}

impl Controller {
    pub async fn new(database: Arc<Database>) -> anyhow::Result<Arc<Self>> {
        let (tx, rx) = mpsc::unbounded_channel();
        let registry = Registry::new();
        let sync = Sync::new(database.clone(), tx.clone());

        let server = Arc::new(Self {
            database,
            registry,
            sync,
            tx,
            rx: Mutex::new(rx),
        });

        {
            let registry = server.registry.clone();
            tokio::spawn(registry.start());
        }

        {
            let sync = server.sync.clone();
            tokio::spawn(sync.start());
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
