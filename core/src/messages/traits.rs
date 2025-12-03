use std::fmt::Debug;

use serde::{Deserialize, Serialize, de::DeserializeOwned};

pub trait Message: Serialize + DeserializeOwned + for<'de> Deserialize<'de> {}

pub trait RequestBody {
    type ResponseBody: Serialize + DeserializeOwned + for<'de> Deserialize<'de>;
}
