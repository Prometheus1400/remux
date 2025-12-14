use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{messages::traits::Message, states::DaemonState};

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct ResponseMessage<T> {
    pub id: Uuid,
    pub result: ResponseResult<T>,
}
impl<T: Serialize + for<'de> Deserialize<'de>> Message for ResponseMessage<T> {}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(tag = "type")]
pub enum ResponseResult<T> {
    Success(T),
    Failure(String),
}

// --------- message bodies ---------  //

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Attach {
    pub initial_daemon_state: DaemonState,
}

// --------- builder ---------  //

pub struct ResultUnset;
pub type ResultSet<T> = ResponseResult<T>;

pub struct ResponseBuilder<ResultState> {
    id: Uuid,
    result: ResultState,
}

impl Default for ResponseBuilder<ResultUnset> {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            result: ResultUnset,
        }
    }
}

impl ResponseBuilder<ResultUnset> {
    pub fn result<T>(self, result: ResponseResult<T>) -> ResponseBuilder<ResultSet<T>> {
        ResponseBuilder { id: self.id, result }
    }
}

impl<T> ResponseBuilder<ResultSet<T>> {
    pub fn build(self) -> ResponseMessage<T> {
        ResponseMessage {
            id: self.id,
            result: self.result,
        }
    }
}
