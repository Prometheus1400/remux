use bytes::Bytes;
use tokio::sync::mpsc;
use tracing::{debug, info, trace};

use crate::{
    actor::{Actor, ActorHandle},
    error::Result,
    pty::{PtyBuilder, PtyHandle}, window::WindowHandle,
};

pub enum PaneEvent {
    UserInput(Bytes),
    PtyOutput(Bytes),
    Render, // uses the diff from prev state to get to desired state (falls back to rerender if no prev state)
    Rerender, // full rerender
    Hide,
    Reveal,
    Kill,
}

pub enum PaneState {
    Visible,
    Hidden,
}

pub struct Pane {
    handle: PaneHandle,
    window_handle: WindowHandle,
    rx: mpsc::Receiver<PaneEvent>,
    pane_state: PaneState,
    // vte related
    vte: vt100::Parser,
    prev_screen_state: Option<vt100::Screen>,
}
impl Pane {
    pub fn new(window_handle: WindowHandle) -> Self {
        let (tx, rx) = mpsc::channel(10);
        let handle = PaneHandle { tx };
        let vte = vt100::Parser::default();
        Self {
            handle,
            window_handle,
            rx,
            vte,
            pane_state: PaneState::Visible,
            prev_screen_state: None,
        }
    }

    async fn handle_pty_output(&mut self, bytes: Bytes) -> Result<()> {
        self.vte.process(&bytes);
        self.handle.render().await.unwrap();
        Ok(())
    }

    async fn handle_render(&mut self) -> Result<()> {
        match &self.prev_screen_state {
            Some(prev) => {
                let cur_screen_state = self.vte.screen().clone();
                let diff = cur_screen_state.state_diff(prev);
                self.prev_screen_state = Some(cur_screen_state);
                self.window_handle.send_pane_output(Bytes::copy_from_slice(&diff)).await;
            }
            None => {self.handle_rerender().await;},
        }
        Ok(())
    }

    async fn handle_rerender(&mut self) -> Result<()> {
        let cur_screen_state = self.vte.screen().clone();
        self.prev_screen_state = Some(cur_screen_state);

            let clear_screen = b"\x1b[H\x1b[2J";
            let new_state = self.vte.screen().state_formatted();
            let output = clear_screen
                .iter()
                .chain(new_state.iter())
                .copied()
                .collect();
        self.window_handle.send_pane_output(output).await;
        return Ok(());
        todo!("don't need to send this when pane is hidden");
        // match self.state {
        //     PaneState::Visible => {
        //     },
        //     PaneState::Hidden => {
        //     }
        // }
    }
}

async fn handle_input(pty_handle: &PtyHandle, bytes: Bytes) -> Result<()> {
    pty_handle.send(bytes).await.unwrap();
    Ok(())
}

impl Actor<PaneHandle> for Pane {
    fn run(mut self) -> Result<PaneHandle> {
        let handle_clone = self.handle.clone();

        let pty_handle = PtyBuilder::new(self.handle.clone()).build().run().unwrap();
        let _task = tokio::spawn({
            async move {
                loop {
                    tokio::select! {
                        Some(event) = self.rx.recv() => {
                            use PaneEvent::*;
                            match event {
                                UserInput(bytes) => {handle_input(&pty_handle, bytes).await.unwrap();}
                                PtyOutput(bytes) => {self.handle_pty_output(bytes).await.unwrap();},
                                Kill =>  { pty_handle.kill().await.unwrap(); break;  }
                                Render => {self.handle_render().await.unwrap();}
                                Rerender => {self.handle_rerender().await.unwrap();}
                                Hide => {self.pane_state = PaneState::Hidden;}
                                Reveal => {self.pane_state = PaneState::Visible;}
                            }
                        }
                    }
                }
            }
        });

        Ok(handle_clone)
    }
}

#[derive(Debug, Clone)]
pub struct PaneHandle {
    tx: mpsc::Sender<PaneEvent>,
}
impl ActorHandle for PaneHandle {
    async fn kill(&self) -> Result<()> {
        self.tx.send(PaneEvent::Kill).await.unwrap();
        Ok(())
    }

    fn is_alive(&self) -> bool {
        !self.tx.is_closed()
    }
}
impl PaneHandle {
    // public api
    pub async fn send_user_input(&self, bytes: Bytes) -> Result<()> {
        self.tx
            .send(PaneEvent::UserInput(bytes))
            .await
            .unwrap();
        Ok(())
    }
    pub async fn send_output_from_pty(&self, bytes: Bytes) -> Result<()> {
        self.tx
            .send(PaneEvent::PtyOutput(bytes))
            .await
            .unwrap();
        Ok(())
    }
    pub async fn rerender(&self) -> Result<()> {
        self.tx.send(PaneEvent::Rerender).await.unwrap();
        Ok(())
    }
    pub async fn hide(&self) -> Result<()> {
        self.tx.send(PaneEvent::Hide).await.unwrap();
        Ok(())
    }
    pub async fn reveal(&self) -> Result<()> {
        self.tx.send(PaneEvent::Reveal).await.unwrap();
        Ok(())
    }
    // for internal use only
    async fn render(&self) -> Result<()> {
        self.tx.send(PaneEvent::Render).await.unwrap();
        Ok(())
    }
}

// // might be needed once ading more pane options
// impl PaneBuilder {
//     pub fn new() -> Self {
//         Self {}
//     }
//
//     pub fn build(self) -> Result<Pane> {
//         let (pty_output_tx, mut pty_output_rx) = mpsc::unbounded_channel::<Bytes>();
//         let (pty_input_tx, mut pty_input_rx) = mpsc::unbounded_channel::<Bytes>();
//         let (closed_tx, closed_rx) = watch::channel(());
//         let (vte_output_tx, _) = broadcast::channel::<Bytes>(1024);
//         let (rerender_tx, mut rerender_rx) = watch::channel(());
//         let vte = Arc::new(Mutex::new(Parser::default()));
//
//         let pty = PtyBuilder::new(pty_output_tx)
//             .with_exit_callback(|| info!("PtyProcess has terminated!"))
//             .with_exit_callback(move || closed_tx.send(()).unwrap())
//             .build();
//
//         // this task feeds the pty output in to the vte
//         let vte_clone = vte.clone();
//         let mut closed_rx_clone = closed_rx.clone();
//         let output_task = tokio::spawn(async move {
//             loop {
//                 tokio::select! {
//                     Ok(_) = closed_rx_clone.changed() => {
//                         break;
//                     },
//                     Some(bytes) = pty_output_rx.recv() => {
//                         let mut guard = vte_clone.lock().await;
//                         trace!("writing pty output {bytes:?} to VTE");
//                         guard.process(&bytes);
//                         rerender_tx.send(()).unwrap();
//                     }
//                 }
//             }
//             debug!("stopping output_task");
//             Ok(())
//         });
//
//         let handler = pty.run().unwrap();
//         let mut closed_rx_clone = closed_rx.clone();
//         let handler_clone = handler.clone();
//         let input_task = tokio::spawn(async move {
//             loop {
//                 tokio::select! {
//                     Ok(_) = closed_rx_clone.changed() => {
//                         break;
//                     },
//                     Some(bytes) = pty_input_rx.recv() => {
//                         if handler_clone.send(bytes).await.is_err() {
//                             break;
//                         }
//                     }
//                 }
//             }
//             debug!("stopping input_task");
//             Ok(())
//         });
//
//         let prev_vte_state: Arc<Mutex<Option<Screen>>> = Arc::new(Mutex::new(None));
//         let vte_clone_2 = vte.clone();
//         let vte_output_tx_clone = vte_output_tx.clone();
//         let prev_vte_state_clone = prev_vte_state.clone();
//         let mut closed_rx_clone = closed_rx.clone();
//         let vte_task = tokio::spawn(async move {
//             let mut interval = tokio::time::interval(Duration::from_millis(1000));
//             loop {
//                 tokio::select! {
//                     Ok(_) = closed_rx_clone.changed() => {
//                         break;
//                     },
//                     _ = interval.tick() => {
//                         trace!("rerendering because of interval");
//                     },
//                     Ok(_) = rerender_rx.changed() => {
//                         trace!("rerendering because of event");
//                     }
//                 };
//                 let bytes = get_keycodes(&vte_clone_2, &prev_vte_state_clone, false).await;
//                 if vte_output_tx_clone.send(bytes).is_err() {
//                     // TODO: maybe some logs or error handling
//                 }
//             }
//             debug!("stopping vte_task");
//             Ok(())
//         });
//
//         // new panes are always focused when created
//         Ok(Pane {
//             pty: handler,
//             input_task,
//             input_tx: pty_input_tx,
//             output_task,
//             closed_rx,
//             vte,
//             vte_task,
//             vte_output_tx,
//             prev_vte_state,
//         })
//     }
// }
//
// pub struct Pane {
//     pty: PtyHandle,
//     // task in charge of sending messages received on 'input_tx' to the PTY process
//     input_task: DaemonTask,
//     // can be borrowed to send input to the pane
//     input_tx: mpsc::UnboundedSender<Bytes>,
//     // task in charge of getting response out of the PTY process and sending it to VTE
//     output_task: DaemonTask,
//     // can be watched to see if the pane has been closed (or underlying PTY process terminated)
//     closed_rx: watch::Receiver<()>,
//     // virtual terminal emulator
//     vte: Arc<Mutex<Parser>>,
//     // task that extracts the VTE output state and sends it to subscribers
//     vte_task: DaemonTask,
//     // kept around for constructing recievers from subscribe
//     vte_output_tx: broadcast::Sender<Bytes>,
//     // previous state of the vte display
//     prev_vte_state: Arc<Mutex<Option<Screen>>>,
// }
//
// // subscribers can subscribe in any state
// impl Pane {
//     pub fn subscribe(&self) -> broadcast::Receiver<Bytes> {
//         // self.output_tx.subscribe()
//         self.vte_output_tx.subscribe()
//     }
//
//     pub async fn redraw(&self) {
//         self.vte_output_tx
//             .send(get_keycodes(&self.vte, &self.prev_vte_state, true).await);
//     }
//
//     pub fn get_sender(&self) -> &mpsc::UnboundedSender<Bytes> {
//         &self.input_tx
//     }
//
//     pub fn get_closed_watcher(&self) -> &watch::Receiver<()> {
//         &self.closed_rx
//     }
// }
//
// async fn get_keycodes(
//     vte: &Arc<Mutex<Parser>>,
//     prev_state: &Arc<Mutex<Option<Screen>>>,
//     force_full_redraw: bool,
// ) -> Bytes {
//     let vte_guard = vte.lock().await;
//     let mut prev = prev_state.lock().await;
//
//     let bytes = match prev.as_ref() {
//         Some(p) if force_full_redraw => vte_guard.screen().state_diff(p),
//         _ => {
//             let clear_screen = b"\x1b[H\x1b[2J";
//             let new_state = vte_guard.screen().state_formatted();
//             clear_screen
//                 .iter()
//                 .chain(new_state.iter())
//                 .copied()
//                 .collect()
//         }
//     };
//     *prev = Some(vte_guard.screen().clone());
//     Bytes::from(bytes)
// }
