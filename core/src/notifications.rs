use bytes::Bytes;
use derive_more::Display;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, PartialEq, Display)]
pub enum ClientNotification {
    #[display("Input(bytes: {{...}})")]
    Input { bytes: Vec<u8> },
    #[display("Resize(rows: {rows}, cols: {cols})")]
    Resize { rows: u16, cols: u16 },
}
