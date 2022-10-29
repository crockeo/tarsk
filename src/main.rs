use std::io::BufRead;
use std::io::BufReader;
use std::io::Read;
use std::io::Write;
use std::net::TcpListener;
use std::net::TcpStream;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

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
	    move || { handle_connections(listener) }
	}),
        thread::spawn({
	    let listener = listener.try_clone()?;
	    move || { connect_to_peers(listener) }
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
