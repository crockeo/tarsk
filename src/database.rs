use std::fs::File;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

use anyhow::anyhow;
use automerge::transaction::Transactable;
use automerge::ActorId;
use automerge::AutoCommit;
use automerge::Change;
use automerge::ChangeHash;
use automerge::ObjId;
use automerge::ObjType;
use chrono::NaiveDate;

pub struct Database {
    doc: Mutex<AutoCommit>,
}

impl Database {
    pub fn new() -> anyhow::Result<Self> {
        let mut doc = AutoCommit::new();
        doc.set_actor(ActorId::random());
        doc.put_object(automerge::ROOT, "tasks", ObjType::List)?;
        Ok(Self {
            doc: Mutex::new(doc),
        })
    }

    pub fn load<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let mut file = File::open(path)?;
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)?;

        Self::from_bytes(&contents)
    }

    pub fn save<P: AsRef<Path>>(&self, path: &Path) -> anyhow::Result<()> {
        let mut file = File::create(path)?;
        file.write_all(&self.to_bytes())?;
        Ok(())
    }

    fn from_bytes(bytes: &[u8]) -> anyhow::Result<Self> {
        let doc = AutoCommit::load(bytes)?;
        Ok(Self {
            doc: Mutex::new(doc),
        })
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut doc = self.doc.lock().unwrap();
        doc.save()
    }

    pub fn get_heads(&self) -> Vec<ChangeHash> {
        let mut doc = self.doc.lock().unwrap();
        doc.get_heads()
    }

    pub fn get_changes(&self, heads: &[ChangeHash]) -> anyhow::Result<Vec<Change>> {
        // TODO: see if there's a good way to do this without cloning everything?
        let mut doc = self.doc.lock().unwrap();
        let changes = doc
            .get_changes(heads)?
            .into_iter()
            .map(Change::clone)
            .collect();
        Ok(changes)
    }

    pub fn apply_changes<T: IntoIterator<Item = Change>>(&self, changes: T) -> anyhow::Result<()> {
        let mut doc = self.doc.lock().unwrap();
        doc.apply_changes(changes)?;
        Ok(())
    }

    pub fn add_task(&self) -> anyhow::Result<Task<'_>> {
        let mut doc = self.doc.lock().unwrap();
        let (_, tasks_id) = doc
            .get(automerge::ROOT, "tasks")?
            .ok_or_else(|| anyhow!("Missing tasks"))?;

        let task_obj_id = doc.insert_object(tasks_id, 0, ObjType::Map)?;
        doc.put_object(&task_obj_id, "title", ObjType::Text)?;
        doc.put_object(&task_obj_id, "body", ObjType::Text)?;

        Ok(Task {
            parent: self,
            task_obj_id,
        })
    }

    pub fn list_tasks(&self) -> anyhow::Result<Vec<Task<'_>>> {
        let doc = self.doc.lock().unwrap();
        let (_, tasks_id) = doc
            .get(automerge::ROOT, "tasks")?
            .ok_or_else(|| anyhow!("Missing tasks"))?;

        let values = doc.values(tasks_id);
        Ok(values
            .into_iter()
            .map(|(_, task_obj_id)| Task {
                parent: self,
                task_obj_id,
            })
            .collect())
    }
}

pub struct Task<'a> {
    parent: &'a Database,
    task_obj_id: ObjId,
}

impl<'a> Task<'a> {
    pub fn image(&self) -> anyhow::Result<TaskImage> {
        let doc = self.parent.doc.lock().unwrap();
        let (_, title_id) = doc
            .get(&self.task_obj_id, "title")?
            .ok_or_else(|| anyhow!("Missing title"))?;

        let (_, body_id) = doc
            .get(&self.task_obj_id, "body")?
            .ok_or_else(|| anyhow!("Missing body"))?;

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
            .ok_or_else(|| anyhow!("Missing title"))?;

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
            .ok_or_else(|| anyhow!("Missing title"))?;
        doc.splice_text(title_id, pos, delete, contents.as_ref())?;
        Ok(())
    }

    pub fn body(&self) -> anyhow::Result<String> {
        let doc = self.parent.doc.lock().unwrap();
        let (_, body_id) = doc
            .get(&self.task_obj_id, "body")?
            .ok_or_else(|| anyhow!("Missing body"))?;

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
            .ok_or_else(|| anyhow!("Missing body"))?;
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
        let database = Database::new().unwrap();

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
    fn test_list_tasks() {
        let database = Database::new().unwrap();
        let task = database.add_task().unwrap();
        task.splice_title(0, 0, "some text").unwrap();

        let tasks = database.list_tasks().unwrap();
        assert_eq!(tasks.len(), 1);
        let task = &tasks[0];
        assert_eq!(task.title().unwrap(), "some text");
    }

    #[test]
    fn test_splice_title() {
        let database = Database::new().unwrap();

        let task = database.add_task().unwrap();
        task.splice_title(0, 0, "hello world!").unwrap();
        assert_eq!(task.title().unwrap(), "hello world!".to_string());
    }

    #[test]
    fn test_splice_body() {
        let database = Database::new().unwrap();

        let task = database.add_task().unwrap();
        task.splice_body(0, 0, "hello world!").unwrap();
        assert_eq!(task.body().unwrap(), "hello world!".to_string());
    }

    #[test]
    fn test_serialization_roundtrip() {
        let bytes = {
            let doc = Database::new().unwrap();
            let task = doc.add_task().unwrap();
            task.splice_title(0, 0, "hello world").unwrap();
            doc.to_bytes()
        };

        let doc = Database::from_bytes(&bytes).unwrap();
        let tasks = doc.list_tasks().unwrap();
        assert_eq!(tasks.len(), 1);
        let task = &tasks[0];
        assert_eq!(task.title().unwrap(), "hello world");
    }
}
