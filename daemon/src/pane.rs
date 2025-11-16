use std::{sync::Arc, time::Duration};

use bytes::Bytes;
use tokio::sync::{Mutex, broadcast, mpsc, watch};
use tracing::{debug, info, trace};
use vt100::{Parser, Screen};

use crate::{
    error::Result,
    pty::{PtyProcessBuilder, PtyProcesss},
    types::NoResTask,
};

pub struct PaneBuilder {}

// might be needed once ading more pane options
impl PaneBuilder {
    pub fn new() -> Self {
        Self {}
    }

    pub fn build(self) -> Result<Pane> {
        let (pty_output_tx, mut pty_output_rx) = mpsc::unbounded_channel::<Bytes>();
        let (pty_input_tx, mut pty_input_rx) = mpsc::unbounded_channel::<Bytes>();
        let (closed_tx, closed_rx) = watch::channel(());
        let (vte_output_tx, _) = broadcast::channel::<Bytes>(1024);
        let (rerender_tx, mut rerender_rx) = watch::channel(());
        let vte = Arc::new(Mutex::new(Parser::default()));

        let pty = PtyProcessBuilder::new(pty_output_tx)
            .with_exit_callback(|| info!("PtyProcess has finished!"))
            .with_exit_callback(move || closed_tx.send(()).unwrap())
            .build()?;

        // this task feeds the pty output in to the vte
        let vte_clone = vte.clone();
        let mut closed_rx_clone = closed_rx.clone();
        let output_task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    Ok(_) = closed_rx_clone.changed() => {
                        break;
                    },
                    Some(bytes) = pty_output_rx.recv() => {
                        let mut guard = vte_clone.lock().await;
                        trace!("writing pty output {bytes:?} to VTE");
                        guard.process(&bytes);
                        rerender_tx.send(()).unwrap();
                    }
                }
            }
            debug!("stopping output_task");
            Ok(())
        });

        let pty_sender = pty.get_sender().clone();
        let mut closed_rx_clone = closed_rx.clone();
        let input_task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    Ok(_) = closed_rx_clone.changed() => {
                        break;
                    },
                    Some(bytes) = pty_input_rx.recv() => {
                        if pty_sender.send(bytes).is_err() {
                            break;
                        }
                    }
                }
            }
            debug!("stopping input_task");
            Ok(())
        });

        let prev_vte_state: Arc<Mutex<Option<Screen>>> = Arc::new(Mutex::new(None));
        let vte_clone_2 = vte.clone();
        let vte_output_tx_clone = vte_output_tx.clone();
        let prev_vte_state_clone = prev_vte_state.clone();
        let mut closed_rx_clone = closed_rx.clone();
        let vte_task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(100));
            loop {
                tokio::select! {
                    Ok(_) = closed_rx_clone.changed() => {
                        break;
                    },
                    _ = interval.tick() => {
                        debug!("rerendering because of interval");
                    },
                    Ok(_) = rerender_rx.changed() => {
                        debug!("rerendering because of event");
                    }
                };
                let bytes = get_keycodes(&vte_clone_2, &prev_vte_state_clone, false).await;
                if vte_output_tx_clone.send(bytes).is_err() {
                    // TODO: maybe some logs or error handling
                }
            }
            debug!("stopping vte_task");
            Ok(())
        });

        // new panes are always focused when created
        Ok(Pane {
            pty,
            input_task,
            input_tx: pty_input_tx,
            output_task,
            closed_rx,
            vte,
            vte_task,
            vte_output_tx,
            prev_vte_state,
        })
    }
}

pub struct Pane {
    pty: PtyProcesss,
    // task in charge of sending messages received on 'input_tx' to the PTY process
    input_task: NoResTask,
    // can be borrowed to send input to the pane
    input_tx: mpsc::UnboundedSender<Bytes>,
    // task in charge of getting response out of the PTY process and sending it to VTE
    output_task: NoResTask,
    // can be watched to see if the pane has been closed (or underlying PTY process terminated)
    closed_rx: watch::Receiver<()>,
    // virtual terminal emulator
    vte: Arc<Mutex<Parser>>,
    // task that extracts the VTE output state and sends it to subscribers
    vte_task: NoResTask,
    // kept around for constructing recievers from subscribe
    vte_output_tx: broadcast::Sender<Bytes>,
    // previous state of the vte display
    prev_vte_state: Arc<Mutex<Option<Screen>>>,
}

// subscribers can subscribe in any state
impl Pane {
    pub fn subscribe(&self) -> broadcast::Receiver<Bytes> {
        // self.output_tx.subscribe()
        self.vte_output_tx.subscribe()
    }

    pub async fn redraw(&self) {
        self.vte_output_tx
            .send(get_keycodes(&self.vte, &self.prev_vte_state, true).await);
    }

    pub fn get_sender(&self) -> &mpsc::UnboundedSender<Bytes> {
        &self.input_tx
    }

    pub fn get_closed_watcher(&self) -> &watch::Receiver<()> {
        &self.closed_rx
    }

    pub fn terminate(self) {}
}

async fn get_keycodes(
    vte: &Arc<Mutex<Parser>>,
    prev_state: &Arc<Mutex<Option<Screen>>>,
    force_full_redraw: bool,
) -> Bytes {
    let vte_guard = vte.lock().await;
    let mut prev = prev_state.lock().await;

    let bytes = match prev.as_ref() {
        Some(p) if force_full_redraw => vte_guard.screen().state_diff(p),
        _ => {
            let clear_screen = b"\x1b[H\x1b[2J";
            let new_state = vte_guard.screen().state_formatted();
            clear_screen
                .iter()
                .chain(new_state.iter())
                .copied()
                .collect()
        }
    };
    *prev = Some(vte_guard.screen().clone());
    Bytes::from(bytes)
}
