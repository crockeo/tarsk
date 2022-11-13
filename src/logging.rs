use std::env::args;
use std::fs::File;
use std::io::Write;
use std::sync::Mutex;

use lazy_static::lazy_static;

pub struct Logger {
    file: Mutex<File>,
}

impl Logger {
    pub fn new() -> anyhow::Result<Self> {
        let args: Vec<String> = args().into_iter().collect();
        let filename = format!("log-{}-{}.txt", args[1], args[2]);
        Ok(Self {
            file: Mutex::new(File::create(filename)?),
        })
    }

    fn log<S: AsRef<str>>(&self, level: &'static str, line: S) {
        let mut file = self.file.lock().unwrap();
        let _ = file.write(format!("{} {}\n", level, line.as_ref()).as_bytes());
    }

    pub fn debug<S: AsRef<str>>(&self, line: S) {
        self.log("DEBUG", line);
    }

    pub fn info<S: AsRef<str>>(&self, line: S) {
        self.log("INFO", line);
    }

    pub fn warn<S: AsRef<str>>(&self, line: S) {
        self.log("WARN", line);
    }

    pub fn error<S: AsRef<str>>(&self, line: S) {
        self.log("ERROR", line)
    }
}

lazy_static! {
    pub static ref GLOBAL: Logger = Logger::new().unwrap();
}
