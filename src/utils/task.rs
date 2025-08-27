use uuid::Uuid;

#[derive(Debug)]
pub struct Task {
    pub id: Uuid,
    pub parent: Uuid,
    pub name: String,
    pub time: u64,
    pub reminder: u32,
    pub done: bool,
    pub reminded: bool,
    pub dued: bool,
}
