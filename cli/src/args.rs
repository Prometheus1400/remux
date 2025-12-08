use clap::{Parser, Subcommand};

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
