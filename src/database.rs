use std::path::Path;
use std::sync::Mutex;

use automerge::AutoCommit;
use chrono::NaiveDate;

pub struct Database {
    doc: Mutex<AutoCommit>,
}

impl Database {
    pub fn new() -> Self {
        todo!()
    }

    pub fn load<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        todo!()
    }

    pub fn save<P: AsRef<Path>>(&self, path: &Path) -> anyhow::Result<()> {
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
