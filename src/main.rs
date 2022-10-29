use std::io::BufRead;
use std::io::BufReader;
use std::io::Read;
use std::io::Write;
use std::net::SocketAddr;
use std::net::TcpListener;
use std::net::TcpStream;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use automerge::transaction::Transactable;
use lazy_static::lazy_static;

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
pub struct AutomergeNumber {
    doc: automerge::AutoCommit,
}

impl AutomergeNumber {
    pub fn new() -> Self {
        let mut doc = automerge::AutoCommit::new();
        doc.set_actor(automerge::ActorId::random());
        Self { doc }
    }

    pub fn load(data: &[u8]) -> anyhow::Result<Self> {
        Ok(Self {
            doc: automerge::AutoCommit::load(data)?,
        })
    }

    pub fn save(&mut self) -> Vec<u8> {
        self.doc.save()
    }

    pub fn merge(&mut self, other: &mut AutomergeNumber) -> anyhow::Result<()> {
        self.doc.merge(&mut other.doc)?;
        Ok(())
    }

    pub fn set_number(&mut self, number: i64) -> anyhow::Result<()> {
        self.doc.put(&automerge::ROOT, "number", number)?;
        self.doc.commit();
        Ok(())
    }

    pub fn get_number(&self) -> anyhow::Result<Option<i64>> {
        let result = self.doc.get(&automerge::ROOT, "number")?;
        if let Some((number, _)) = result {
            Ok(Some(number.to_i64().unwrap()))
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod automerge_number_tests {
    use super::*;

    #[test]
    fn test_get_number_empty() -> anyhow::Result<()> {
        let number = AutomergeNumber::new();
        assert_eq!(number.get_number()?, None);
        Ok(())
    }

    #[test]
    fn test_set_get_number() -> anyhow::Result<()> {
        let mut number = AutomergeNumber::new();
        number.set_number(1234)?;
        assert_eq!(number.get_number()?, Some(1234));
        Ok(())
    }

    #[test]
    fn test_set_get_number_multiple() -> anyhow::Result<()> {
        let mut number = AutomergeNumber::new();
        number.set_number(1234)?;
        number.set_number(5678)?;
        assert_eq!(number.get_number()?, Some(5678));
        Ok(())
    }

    #[test]
    fn test_sync() -> anyhow::Result<()> {
        let mut num1 = AutomergeNumber::new();
        let mut num2 = AutomergeNumber::new();

        num1.set_number(1234)?;
        assert_eq!(num1.get_number()?, Some(1234));
        assert_eq!(num2.get_number()?, None);

        num2.merge(&mut num1)?;
        assert_eq!(num1.get_number()?, Some(1234));
        assert_eq!(num2.get_number()?, Some(1234));

        Ok(())
    }

    #[test]
    fn test_sync_multiple() -> anyhow::Result<()> {
        let mut num1 = AutomergeNumber::new();
        let mut num2 = AutomergeNumber::new();
        let mut num3 = AutomergeNumber::new();

        num1.set_number(1234)?;
        assert_eq!(num1.get_number()?, Some(1234));
        assert_eq!(num2.get_number()?, None);
        assert_eq!(num3.get_number()?, None);

        num2.merge(&mut num1)?;
        assert_eq!(num1.get_number()?, Some(1234));
        assert_eq!(num2.get_number()?, Some(1234));
        assert_eq!(num3.get_number()?, None);

        num3.set_number(5678)?;
        assert_eq!(num1.get_number()?, Some(1234));
        assert_eq!(num2.get_number()?, Some(1234));
        assert_eq!(num3.get_number()?, Some(5678));

        Ok(())
    }
}

// 2. make a peering server which periodically syncs up the content
//    shared inside of the automerge implementation
pub struct SyncServer {
    listener: TcpListener,
    peer: SocketAddr,
    number: Mutex<AutomergeNumber>,
}

impl SyncServer {
    pub fn new() -> anyhow::Result<Arc<Self>> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        teprintln!("Listening on {}...", listener.local_addr()?);

        let mut raw_peer = String::new();
        let mut stdin = BufReader::new(std::io::stdin());
        stdin.read_line(&mut raw_peer)?;
        let peer = SocketAddr::from_str(raw_peer.trim())?;

        let sync_server = Arc::new(SyncServer {
            listener,
            peer,
            number: Mutex::new(AutomergeNumber::new()),
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
                        teprintln!("PUSH error: {}", e);
                    }
                    thread::sleep(Duration::from_millis(100));
                }
            });
        }

        Ok(sync_server)
    }

    pub fn set_number(&self, number: i64) -> anyhow::Result<()> {
        let mut doc = self.number.lock().unwrap();
        doc.set_number(number)
    }

    pub fn get_number(&self) -> anyhow::Result<Option<i64>> {
        let doc = self.number.lock().unwrap();
        doc.get_number()
    }

    fn pull(self: &Arc<Self>) -> anyhow::Result<()> {
        let (mut stream, _) = self.listener.accept()?;

        let mut contents = vec![];
        stream.read_to_end(&mut contents)?;

        let mut other_doc = AutomergeNumber::load(&contents)?;
        let mut doc = self.number.lock().unwrap();
        doc.merge(&mut other_doc)?;

        Ok(())
    }

    fn push(self: &Arc<Self>) -> anyhow::Result<()> {
        let mut stream = TcpStream::connect(self.peer)?;
        let buf = {
            let mut doc = self.number.lock().unwrap();
            doc.save()
        };
        stream.write_all(&buf)?;
        Ok(())
    }
}

// 3. make a user interface that lets people modify the content
//    inside of the automerge document
fn main() -> anyhow::Result<()> {
    let sync_server = SyncServer::new()?;
    let mut stdin = BufReader::new(std::io::stdin());
    loop {
        let mut line = String::new();
        stdin.read_line(&mut line)?;

        if let Ok(num) = i64::from_str(&line.trim()) {
            sync_server.set_number(num)?;
        }
        println!("Current value: {:?}", sync_server.get_number()?);
    }
}
