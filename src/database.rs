use std::fs::File;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

use automerge::ActorId;
use automerge::AutoCommit;
use chrono::NaiveDate;

pub struct Database {
    doc: Mutex<AutoCommit>,
}

impl Database {
    pub fn new() -> Self {
	let mut doc = AutoCommit::new();
	doc.set_actor(ActorId::random());
        Self {
            doc: Mutex::new(doc),
        }
    }

    pub fn load<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let mut file = File::open(path)?;
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)?;

        let doc = AutoCommit::load(&contents)?;
        Ok(Self {
            doc: Mutex::new(doc),
        })
    }

    pub fn save<P: AsRef<Path>>(&self, path: &Path) -> anyhow::Result<()> {
        let mut doc = self.doc.lock().unwrap();
        let contents = doc.save();

        let mut file = File::create(path)?;
        file.write_all(&contents)?;
        Ok(())
    }

    pub fn add_task<'a>(&'a self) -> anyhow::Result<Task<'a>> {
	todo!()
    }

    pub fn list_tasks<'a>(&'a self) -> anyhow::Result<Vec<Task<'a>>> {
        todo!()
    }

    pub fn get_task<'a>(&'a self, task_id: String) -> anyhow::Result<Option<Task<'a>>> {
        todo!()
    }
}

pub enum TaskField {
    Title,
    Scheduled,
    Body,
}

pub struct Task<'a> {
    parent: &'a Database,
    task_id: String,
}

impl<'a> Task<'a> {
    pub fn image(&self) -> anyhow::Result<TaskImage> {
        todo!()
    }

    pub fn title(&self) -> anyhow::Result<String> {
        todo!()
    }

    pub fn splice_title<S: AsRef<str>>(
        &self,
        insert_pos: usize,
        delete: usize,
        contents: S,
    ) -> anyhow::Result<()> {
        todo!()
    }

    pub fn scheduled(&self) -> anyhow::Result<Option<NaiveDate>> {
        todo!()
    }

    pub fn schedule(&self, day: Option<NaiveDate>) -> anyhow::Result<()> {
        todo!()
    }

    pub fn body(&self) -> anyhow::Result<String> {
        todo!()
    }

    pub fn splice_body<S: AsRef<str>>(
        &self,
        insert_pos: usize,
        delete: usize,
        contents: S,
    ) -> anyhow::Result<()> {
        todo!()
    }
}

pub struct TaskImage {
    pub task_id: String,
    pub title: String,
    pub scheduled: Option<NaiveDate>,
    pub body: String,
}
