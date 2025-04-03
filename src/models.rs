use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Todo {
    pub id: u32,
    pub title: String,
    pub completed: bool,
}

#[derive(Serialize, Deserialize)]
pub struct Category {
    pub id: u32,
    pub name: String,
    pub todos: Vec<Todo>,
}
