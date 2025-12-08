use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    messages::{
        response,
        traits::{Message, RequestBody},
    },
    rand,
};

// --------- serialized from the client ---------  //

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct CliRequestMessage<T: RequestBody> {
    pub id: u32,
    pub body: T,
}
impl<T: RequestBody + Serialize + for<'de> Deserialize<'de>> Message for CliRequestMessage<T> {}

// --------- deserialized in the daemon ---------  //

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct DaemonRequestMessage {
    pub id: u32,
    pub body: DaemonRequestMessageBody,
}
#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(untagged)]
pub enum DaemonRequestMessageBody {
    Attach(Attach),
}
impl Message for DaemonRequestMessage {}

// --------- message bodies ---------  //

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Attach {
    pub id: Uuid,
    pub session_id: u32,
    pub create: bool,
}
impl RequestBody for Attach {
    type ResponseBody = response::Attach;
}

// --------- builder ---------  //

pub struct BodyUnset;
pub type BodySet<T> = T;

#[derive(Debug)]
pub struct RequestBuilder<BodyState> {
    id: u32,
    body: BodyState,
}

impl Default for RequestBuilder<BodyUnset> {
    fn default() -> Self {
        Self {
            id: rand::generate_id(),
            body: BodyUnset,
        }
    }
}

impl RequestBuilder<BodyUnset> {
    pub fn body<T: RequestBody>(self, body: T) -> RequestBuilder<BodySet<T>> {
        RequestBuilder { id: self.id, body }
    }
}

impl<T: RequestBody> RequestBuilder<BodySet<T>> {
    pub fn build(self) -> CliRequestMessage<T> {
        CliRequestMessage {
            id: self.id,
            body: self.body,
        }
    }
}
