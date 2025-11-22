use derive_more::Display;
use serde::{Deserialize, Serialize};

pub use crate::error::{Error, Result};

pub trait Message {
    fn get_id(&self) -> u32;
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Display)]
#[display("request({id}, {body})")]
pub struct RequestMessage {
    pub id: u32,
    pub body: RequestBody,
}
impl RequestMessage {
    pub fn new(id: u32, body: RequestBody) -> Self {
        Self { id, body }
    }
    pub fn body(body: RequestBody) -> Self {
        let id = 1; // TODO: make this randomly generated
        Self { id, body }
    }
}
impl Message for RequestMessage {
    fn get_id(&self) -> u32 {
        self.id
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Display)]
#[display("response({id}, {body})")]
pub struct ResponseMessage {
    id: u32,
    pub body: ResponseBody,
}
impl ResponseMessage {
    pub fn new(id: u32, body: ResponseBody) -> Self {
        Self { id, body }
    }
}
impl Message for ResponseMessage {
    fn get_id(&self) -> u32 {
        self.id
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Display)]
#[serde(tag = "type")]
pub enum RequestBody {
    #[display("Attach: {{session_id: {session_id}}}")]
    Attach {
        session_id: u32,
    },
    // session commands
    SessionsList,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Display)]
#[serde(tag = "type")]
pub enum ResponseBody {
    #[display("sessions: {sessions:?}")]
    SessionsList { sessions: Vec<u32> },
}
