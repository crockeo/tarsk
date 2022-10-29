use std::io::BufRead;
use std::io::BufReader;
use std::io::Read;
use std::io::Write;
use std::net::TcpListener;
use std::net::TcpStream;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use automerge::transaction::Transactable;

// high-level idea for v0 concept
//
// 2. make a peering server which periodically syncs up the content
//    shared inside of the automerge implementation
//
// 3. make a user interface that lets people modify the content
//    inside of the automerge document

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

        num3.merge(&mut num1)?;
        num1.merge(&mut num3)?;
        assert_eq!(num1.get_number()?, Some(1234));
        assert_eq!(num2.get_number()?, Some(1234));
        assert_eq!(num3.get_number()?, Some(1234));

        Ok(())
    }
}

use lazy_static::lazy_static;

const SPECIAL_SAUCE: &'static str = "TaRsK\n";

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

fn main() -> anyhow::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    teprintln!("Listening on {}...", listener.local_addr()?);

    let threads = vec![
        thread::spawn({
            let listener = listener.try_clone()?;
            move || handle_connections(listener)
        }),
        thread::spawn({
            let listener = listener.try_clone()?;
            move || connect_to_peers(listener)
        }),
    ];
    for thread in threads.into_iter() {
        _ = thread.join();
    }
    Ok(())
}

fn handle_connections(listener: TcpListener) -> anyhow::Result<()> {
    for stream in listener.incoming() {
        let stream = stream?;
        thread::spawn(move || -> anyhow::Result<()> { handle_stream(stream) });
    }
    Ok(())
}

fn handle_stream(mut stream: TcpStream) -> anyhow::Result<()> {
    let addr = stream.peer_addr()?;
    teprintln!("Handling {}...", addr);

    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    while let Ok(read_size) = reader.read_line(&mut line) {
        if read_size == 0 {
            break;
        }

        if line == SPECIAL_SAUCE {
            stream.write_all(SPECIAL_SAUCE.as_bytes())?;
        }

        line.clear();
    }

    teprintln!("Lost connection to {}...", addr);
    Ok(())
}

fn connect_to_peers(listener: TcpListener) -> anyhow::Result<()> {
    let self_port = listener.local_addr()?.port();

    teprintln!("Connecting to peers...");

    let mut peers = vec![];
    for port in 1024..=u16::MAX {
        if port == self_port {
            continue;
        }

        let mut stream = match TcpStream::connect(format!("127.0.0.1:{}", port)) {
            Err(_) => continue,
            Ok(stream) => stream,
        };

        let timeout = Some(Duration::from_millis(50));
        stream.set_read_timeout(timeout)?;
        stream.set_write_timeout(timeout)?;

        if let Err(_) = stream.write_all(SPECIAL_SAUCE.as_bytes()) {
            continue;
        }

        let mut buf: [u8; 6] = [0; 6];
        if let Err(_) = stream.read_exact(&mut buf) {
            continue;
        }

        if buf != SPECIAL_SAUCE.as_bytes() {
            continue;
        }
        peers.push(stream.peer_addr()?);
    }
    teprintln!("{:?}", peers);

    Ok(())
}
