use bytes::Bytes;
use tokio::{
    sync::{broadcast, mpsc, watch},
    task::JoinHandle,
};
use tracing::{debug, info};

use crate::{
    error::{Error, Result},
    pty::{PtyProcessBuilder, PtyProcesss}, types::NoResTask,
};

pub struct PaneBuilder {
    output_tx: broadcast::Sender<Bytes>,
}

// might be needed once ading more pane options
impl PaneBuilder {
    pub fn new() -> Self {
        // TODO: can be moved to build
        let (output_tx, _) = broadcast::channel(16);
        Self { output_tx }
    }

    pub fn build(self) -> Result<Pane<Focused>> {
        let (pty_output_tx, mut pty_output_rx) = mpsc::unbounded_channel::<Bytes>();
        let (pty_input_tx, mut pty_input_rx) = mpsc::unbounded_channel::<Bytes>();
        let (closed_tx, closed_rx) = watch::channel(false);

        let pty = PtyProcessBuilder::new(pty_output_tx, closed_tx)
            .with_exit_callback(|| info!("PtyProcess has finished!"))
            .build()?;

        let output_tx_clone = self.output_tx.clone();
        let output_task = tokio::spawn(async move {
            while let Some(bytes) = pty_output_rx.recv().await {
                if output_tx_clone.send(bytes).is_err() {
                    // TODO: maybe some trace log
                }
            }
            Ok(())
        });

        let pty_sender = pty.get_sender().clone();
        let input_task = tokio::spawn(async move {
            while let Some(bytes) = pty_input_rx.recv().await {
                if pty_sender.send(bytes).is_err() {
                    break;
                }
            }
            debug!("terminating pane input task!");
            Ok(())
        });

        // new panes are always focused when created
        Ok(Pane::new(
            pty,
            input_task,
            pty_input_tx,
            output_task,
            self.output_tx,
            closed_rx,
        ))
    }
}

pub trait PaneState {}
// pub trait IsFocused : PaneState {}
// pub trait IsHidden : PaneState {}

pub struct Focused {}
impl PaneState for Focused {}
// impl IsFocused for Focused {}

pub struct Hidden {}
impl PaneState for Hidden {}
// impl IsHidden for Hidden {}

pub struct Pane<State> {
    pty: PtyProcesss,
    input_task: NoResTask,
    input_tx: mpsc::UnboundedSender<Bytes>, // this we use to let others send input to the pane
    output_task: NoResTask,
    output_tx: broadcast::Sender<Bytes>, // keep this around to construct receivers for 'subscribe'
    closed_rx: watch::Receiver<bool>,
    _state: std::marker::PhantomData<State>,
}

// subscribers can subscribe in any state
impl<State> Pane<State> {
    pub fn subscribe(&self) -> broadcast::Receiver<Bytes> {
        self.output_tx.subscribe()
    }

    pub fn get_sender(&self) -> &mpsc::UnboundedSender<Bytes> {
        &self.input_tx
    }

    pub fn get_closed_watcher(&self) -> &watch::Receiver<bool> {
        &self.closed_rx
    }
}

impl Pane<Focused> {
    // we can only construct a pane in focused state
    fn new(
        pty: PtyProcesss,
        input_task: NoResTask,
        input_tx: mpsc::UnboundedSender<Bytes>,
        output_task: NoResTask,
        output_tx: broadcast::Sender<Bytes>,
        closed_rx: watch::Receiver<bool>,
    ) -> Self {
        Self {
            pty,
            input_task,
            input_tx,
            output_task,
            output_tx,
            closed_rx,
            _state: std::marker::PhantomData,
        }
    }

    pub fn hide(self) -> Pane<Hidden> {
        Pane::<Hidden> {
            pty: self.pty,
            input_task: self.input_task,
            input_tx: self.input_tx,
            output_task: self.output_task,
            output_tx: self.output_tx,
            closed_rx: self.closed_rx,
            _state: std::marker::PhantomData,
        }
    }
}

impl Pane<Hidden> {
    pub fn focus(self) -> Pane<Focused> {
        Pane::<Focused> {
            pty: self.pty,
            input_task: self.input_task,
            input_tx: self.input_tx,
            output_task: self.output_task,
            output_tx: self.output_tx,
            closed_rx: self.closed_rx,
            _state: std::marker::PhantomData,
        }
    }
}
