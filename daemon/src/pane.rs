use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use tokio::{
    sync::{
        Mutex,
        mpsc::{self, UnboundedReceiver, UnboundedSender},
    },
    task::JoinHandle,
};
use tracing::info;

use crate::{
    error::{Error, Result},
    pty::{PtyProcessBuilder, PtyProcesss}, traits::Active,
};

type Task = JoinHandle<std::result::Result<(), Error>>;

const NEWLINE_CHAR: char = '\n';

pub struct PaneBuilder {
    history_size: usize,
    live_bytes_tx: UnboundedSender<u8>,
}

impl PaneBuilder {
    pub fn new(history_size: usize, live_bytes_tx: UnboundedSender<u8>) -> Self {
        Self {
            history_size,
            live_bytes_tx,
        }
    }

    pub fn build(self) -> Result<Pane<Focused>> {
        let (pty_output_tx, mut pty_output_rx) = mpsc::unbounded_channel::<u8>();
        let history = Arc::new(Mutex::new(VecDeque::new()));
        let live_view = Arc::new(AtomicBool::new(true));
        let pty = PtyProcessBuilder::new(pty_output_tx)
            .with_exit_callback(|| info!("hi"))
            .build()?;

        let history_clone = history.clone();
        let live_view_clone = live_view.clone();
        let output_task = tokio::spawn(async move {
            let mut line = String::new();
            while let Some(byte) = pty_output_rx.recv().await {
                // if the pane is not hidden we should send updates
                if live_view_clone.load(Ordering::Acquire) {
                    self.live_bytes_tx.send(byte).map_err(|_| {
                        Error::Custom("couldn't send byte to live_bytes_tx".to_owned())
                    })?;
                }
                match byte as char {
                    NEWLINE_CHAR => {
                        line.push(NEWLINE_CHAR);
                        history_clone.lock().await.push_front(line.clone());
                        line.clear();
                    }
                    c => {
                        line.push(c);
                    }
                }
            }
            Ok(())
        });

        // new panes are always focused when created
        Ok(Pane {
            history_size: self.history_size,
            history,
            live_view,
            output_task,
            pty,
            _state: std::marker::PhantomData,
        })
    }
}

// TODO: figure out some way to make only one pane able to have focusable state

pub struct Focused {}
impl Active for Focused {}
// pub struct Visible {} // TODO
// impl Active for Visible {}
pub struct Hidden {}


pub struct Pane<State> {
    history_size: usize,
    history: Arc<Mutex<VecDeque<String>>>,
    pty: PtyProcesss,
    output_task: Task,
    live_view: Arc<AtomicBool>, // this must match the state - TODO: figure out a better way to do this
    _state: std::marker::PhantomData<State>,
}

impl<State> Pane<State> {
    pub async fn get_history(&mut self, lines: usize) -> Vec<String> {
        let guard = self.history.lock().await;
        let len = guard.len().saturating_sub(lines);
        guard.iter().skip(len).cloned().collect()
    }
}

impl<State: Active> Pane<State> {
    pub fn hide(self) -> Pane<Hidden> {
        self.live_view.store(false, Ordering::Release);

        Pane::<Hidden> {
            history_size: self.history_size,
            history: self.history,
            live_view: self.live_view,
            pty: self.pty,
            output_task: self.output_task,
            _state: std::marker::PhantomData,
        }
    }
}

// impl Pane<Focused> {
//     pub fn hide(self) -> Pane<Hidden> {
//         self.live_view.store(false, Ordering::Release);
//
//         Pane::<Hidden> {
//             history_size: self.history_size,
//             history: self.history,
//             live_view: self.live_view,
//             pty: self.pty,
//             output_task: self.output_task,
//             _state: std::marker::PhantomData,
//         }
//     }
// }


// impl Pane<Visible> {}

impl Pane<Hidden> {
    pub fn focus(self) -> Pane<Focused> {
        self.live_view.store(true, Ordering::Release);

        Pane::<Focused> {
            history_size: self.history_size,
            history: self.history,
            live_view: self.live_view,
            pty: self.pty,
            output_task: self.output_task,
            _state: std::marker::PhantomData,
        }
    }
}
