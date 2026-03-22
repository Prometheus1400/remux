use serde::{Deserialize, Serialize, de::DeserializeOwned};

pub trait Message: Serialize + DeserializeOwned + for<'de> Deserialize<'de> {}

pub trait RequestBody {
    type ResponseBody: Serialize + DeserializeOwned + for<'de> Deserialize<'de>;

    fn to_daemon_request_body(&self) -> crate::messages::request::DaemonRequestMessageBody;
}
