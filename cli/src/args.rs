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
        session_name: String,
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
