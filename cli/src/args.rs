use clap::{Parser, Subcommand};
use remux_core::messages::{CliRequestMessage, RequestBody, RequestBuilder, request};

#[derive(Parser, Debug)]
pub struct Args {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    Attach {
        #[arg(short = 's', long = "session")]
        session_id: u32,
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

impl Commands {
    pub fn into_request(self) -> CliRequestMessage<impl RequestBody> {
        match self {
            Self::Attach { session_id } => RequestBuilder::default()
                .body(request::Attach {
                    session_id,
                    create: true,
                })
                .build(),
            Self::Session { .. } => todo!(),
        }
    }
}

// #[allow(clippy::from_over_into)]
// impl<T: RequestBody> Into<RequestMessage<T>> for Commands {
//     fn into(self) -> RequestMessage<T> {
//         match self {
//             Self::Attach { session_id } => RequestBuilder::default()
//                 .body(request::Attach {
//                     session_id,
//                     create: true,
//                 })
//                 .build(),
//             Self::Session { action } => action.into(),
//         }
//     }
// }

// #[allow(clippy::from_over_into)]
// impl Into<RequestBody> for SessionCommands {
//     fn into(self) -> RequestBody {
//         match self {
//             SessionCommands::List => RequestBody::SessionsList,
//         }
//     }
// }
// #[allow(clippy::from_over_into)]
// impl Into<RequestBody> for Commands {
//     fn into(self) -> RequestBody {
//         let body: RequestBody = self.into();
//         RequestBuilder::default().body(body).build()
//     }
// }
// #[allow(clippy::from_over_into)]
// impl Into<RequestBody> for SessionCommands {
//     fn into(self) -> RequestBody {
//         let body: RequestBody = self.into();
//         RequestBuilder::default().body(body).build()
//     }
// }
