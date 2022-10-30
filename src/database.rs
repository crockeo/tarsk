use std::fs::File;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

use anyhow::anyhow;
use automerge::transaction::Transactable;
use automerge::ActorId;
use automerge::AutoCommit;
use automerge::ObjId;
use automerge::ObjType;
use chrono::NaiveDate;
use uuid::Uuid;

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
        let mut doc = self.doc.lock().unwrap();
        let task_uuid = Uuid::new_v4();

        let task_obj_id = doc.put_object(automerge::ROOT, task_uuid.to_string(), ObjType::Map)?;
	doc.put_object(&task_obj_id, "title", ObjType::Text)?;
	doc.put_object(&task_obj_id, "body", ObjType::Text)?;

        Ok(Task {
            parent: self,
            task_uuid,
            task_obj_id,
        })
    }

    pub fn list_tasks<'a>(&'a self) -> anyhow::Result<Vec<Task<'a>>> {
        todo!()
    }

    pub fn get_task<'a>(&'a self, task_uuid: Uuid) -> anyhow::Result<Option<Task<'a>>> {
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
    task_uuid: Uuid,
    task_obj_id: ObjId,
}

impl<'a> Task<'a> {
    pub fn image(&self) -> anyhow::Result<TaskImage> {
        let doc = self.parent.doc.lock().unwrap();
        let (_, title_id) = doc
            .get(&self.task_obj_id, "title")?
            .ok_or(anyhow!("Missing title"))?;

        let (_, body_id) = doc
            .get(&self.task_obj_id, "body")?
            .ok_or(anyhow!("Missing body"))?;

        Ok(TaskImage {
	    title: doc.text(title_id)?,
            scheduled: None,
	    body: doc.text(body_id)?,
        })
    }

    pub fn title(&self) -> anyhow::Result<String> {
        let doc = self.parent.doc.lock().unwrap();
        let (_, title_id) = doc
            .get(&self.task_obj_id, "title")?
            .ok_or(anyhow!("Missing title"))?;

	Ok(doc.text(title_id)?)
    }

    pub fn splice_title<S: AsRef<str>>(
        &self,
        pos: usize,
        delete: usize,
        contents: S,
    ) -> anyhow::Result<()> {
        let mut doc = self.parent.doc.lock().unwrap();
        let (_, title_id) = doc
            .get(&self.task_obj_id, "title")?
            .ok_or(anyhow!("Missing title"))?;
        doc.splice_text(title_id, pos, delete, contents.as_ref())?;
        Ok(())
    }

    pub fn body(&self) -> anyhow::Result<String> {
        let doc = self.parent.doc.lock().unwrap();
        let (_, body_id) = doc
            .get(&self.task_obj_id, "body")?
            .ok_or(anyhow!("Missing body"))?;

	Ok(doc.text(body_id)?)
    }

    pub fn splice_body<S: AsRef<str>>(
        &self,
        pos: usize,
        delete: usize,
        contents: S,
    ) -> anyhow::Result<()> {
        let mut doc = self.parent.doc.lock().unwrap();
        let (_, body_id) = doc
            .get(&self.task_obj_id, "body")?
            .ok_or(anyhow!("Missing body"))?;
        doc.splice_text(body_id, pos, delete, contents.as_ref())?;
        Ok(())
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct TaskImage {
    pub title: String,
    pub scheduled: Option<NaiveDate>,
    pub body: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_task() {
        let database = Database::new();

        let task = database.add_task().unwrap();
        let task_image = task.image().unwrap();
        assert_eq!(
            task_image,
            TaskImage {
                title: "".to_string(),
                scheduled: None,
                body: "".to_string(),
            }
        );
    }

    #[test]
    fn test_splice_title() {
	let database = Database::new();

	let task = database.add_task().unwrap();
	task.splice_title(0, 0, "hello world!").unwrap();
	assert_eq!(task.title().unwrap(), "hello world!".to_string());
    }

    #[test]
    fn test_splice_body() {
	let database = Database::new();

	let task = database.add_task().unwrap();
	task.splice_body(0, 0, "hello world!").unwrap();
	assert_eq!(task.body().unwrap(), "hello world!".to_string());
    }
}
