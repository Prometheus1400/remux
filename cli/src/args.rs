use clap::{Parser, Subcommand};
use remux_core::messages::{RequestBody, RequestMessage};

#[derive(Parser, Debug)]
pub struct Args {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    Attach {
        #[arg(short = 's', long = "session")]
        session_id: u16,
    },
    Session {
        #[command(subcommand)]
        action: SessionCommands,
    },
}

#[derive(Subcommand, Debug)]
pub enum SessionCommands {
    List,
}

#[allow(clippy::from_over_into)]
impl Into<RequestBody> for Commands {
    fn into(self) -> RequestBody {
        match self {
            Self::Attach { session_id } => RequestBody::Attach { session_id },
            Self::Session { action } => action.into(),
        }
    }
}
#[allow(clippy::from_over_into)]
impl Into<RequestBody> for SessionCommands {
    fn into(self) -> RequestBody {
        match self {
            SessionCommands::List => RequestBody::SessionsList,
        }
    }
}
#[allow(clippy::from_over_into)]
impl Into<RequestMessage> for Commands {
    fn into(self) -> RequestMessage {
        let body: RequestBody = self.into();
        RequestMessage::body(body)
    }
}
#[allow(clippy::from_over_into)]
impl Into<RequestMessage> for SessionCommands {
    fn into(self) -> RequestMessage {
        let body: RequestBody = self.into();
        RequestMessage::body(body)
    }
}
