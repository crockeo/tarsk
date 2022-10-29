use std::io::BufRead;
use std::io::BufReader;
use std::io::Read;
use std::io::Write;
use std::net::SocketAddr;
use std::net::TcpListener;
use std::net::TcpStream;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use lazy_static::lazy_static;

const SPECIAL_SAUCE: &'static str = "TaRsK";

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
    let threads = vec![
        thread::spawn(handle_connections),
        thread::spawn(connect_to_peers),
    ];
    for thread in threads.into_iter() {
        _ = thread.join();
    }
    Ok(())
}

fn handle_connections() -> anyhow::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    {
        let _ = PRINT_LOCK.lock().unwrap();
        teprintln!("Listening on {}...", listener.local_addr()?);
    }
    for stream in listener.incoming() {
        let stream = stream?;
        thread::spawn(move || -> anyhow::Result<()> { handle_stream(stream) });
    }
    Ok(())
}

fn handle_stream(stream: TcpStream) -> anyhow::Result<()> {
    let addr = stream.peer_addr()?;
    teprintln!("Handling {}...", addr);

    let mut buf = BufReader::new(stream);
    let mut line = String::new();
    while let Ok(read_size) = buf.read_line(&mut line) {
        if read_size == 0 {
            break;
        }
        print!("{}", line);
        line.clear();
    }

    teprintln!("Lost connection to {}...", addr);
    Ok(())
}

fn connect_to_peers() -> anyhow::Result<()> {
    teprintln!("Connecting to peers...");

    let mut peers = vec![];
    for port in 1024..=u16::MAX {
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

        let mut buf: [u8; 5] = [0; 5];
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
