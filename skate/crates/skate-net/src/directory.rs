//! Bounded control messages between the game and the optional discovery helper.
use serde::{Deserialize, Serialize};
pub const NAMESPACE: &str = "skate3rust-free-skate-v5";
// Spacewar is shared with other games; identify ours without filtering versions.
pub fn is_game_lobby(namespace: &str) -> bool {
    namespace.starts_with("skate3rust-free-skate-v")
}
pub const PAGE_SIZE: usize = 5;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    pub command: Command,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Command {
    Browse {
        page: usize,
        map: u64,
        physics: u64,
    },
    Host {
        map: u64,
        physics: u64,
        label: String,
    },
    Join {
        lobby: u64,
        map: u64,
        physics: u64,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Row {
    pub id: u64,
    pub map: String,
    pub players: usize,
    pub capacity: usize,
    pub compatible: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Event {
    Rows {
        page: usize,
        total: usize,
        rows: Vec<Row>,
    },
    Owner {
        lobby: u64,
        owner: u64,
        own: u64,
    },
    Error(String),
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Response {
    pub request: u64,
    pub event: Event,
}
pub fn encode<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("Control serialization")
}
pub fn request(text: &str) -> Option<Request> {
    if text.len() > 1200 {
        return None;
    }
    let r: Request = serde_json::from_str(text).ok()?;
    match &r.command {
        Command::Browse { page, .. } if *page > 100 => return None,
        Command::Host { label, .. } if label.len() > 64 || label.contains('\0') => return None,
        _ => (),
    };
    Some(r)
}
pub fn response(text: &str) -> Option<Response> {
    if text.len() > 1400 {
        return None;
    }
    let r: Response = serde_json::from_str(text).ok()?;
    match &r.event {
        Event::Rows { rows, .. }
            if rows.len() > PAGE_SIZE
                || rows
                    .iter()
                    .any(|r| r.map.len() > 64 || r.capacity > 10 || r.players > 10) =>
        {
            return None;
        }
        Event::Owner { lobby, owner, own } if *lobby == 0 || *owner == 0 || *own == 0 => {
            return None;
        }
        _ => (),
    };
    Some(r)
}

/// Keep labels readable and bounded in bytes, including Unicode map names.
pub fn label(text: &str) -> String {
    let mut result = String::new();
    for c in text.chars().filter(|c| !c.is_control()).take(32) {
        if result.len() + c.len_utf8() > 48 {
            break;
        }
        result.push(c);
    }
    result
}
