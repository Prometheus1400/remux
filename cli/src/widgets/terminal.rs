// // use bytes::Bytes;
// // use ratatui::layout::Rect;
// // use tui_term::widget::PseudoTerminal;
// // use vt100::Parser;
// //
// // use crate::widgets::traits::Renderable;
// //
// // #[derive(Default)]
// // pub struct TerminalWidget {
// //     emulator: Parser,
// // }
// //
// // impl TerminalWidget {
// //     pub fn write(&mut self, bytes: Bytes) {
// //         self.emulator.process(&bytes);
// //     }
// //
// //     pub fn size(&self) -> (u16, u16) {
// //         self.emulator.screen().size()
// //     }
// //
// //     pub fn set_size(&mut self, rows: u16, cols: u16) {
// //         self.emulator.set_size(rows, cols);
// //     }
// //
// // }
// //
// // impl Renderable for TerminalWidget {
// //     fn render(&self, f: &mut ratatui::Frame, rect: Rect) {
// //         f.render_widget(PseudoTerminal::new(self.emulator.screen()), rect);
// //     }
// // }
//
// use std::time::Duration;
//
// use bytes::Bytes;
// use handle_macro::Handle;
// use remux_core::{
//     comm,
//     events::{CliEvent, DaemonEvent},
//     states::DaemonState,
// };
// use tokio::{io::AsyncReadExt, net::UnixStream, sync::mpsc, time::interval};
// use tracing::{Instrument, debug};
//
// use crate::{
//     input_parser::{Action, InputParser, ParsedEvent},
//     prelude::*,
//     utils::DisplayableVec,
// };
//
// #[derive(Handle)]
// pub enum TerminalWidgetEvent {
//     Selected(Option<usize>), // index of the selected item
// }
// use TerminalWidgetEvent::*;
//
// #[derive(Debug)]
// enum ClientWidgetEvent {
//     Normal,           // running normally with stdin parsed into events and sent to daemon
//     SelectingSession, // means that the ui is currently busy selecting redirects stdin to ui selector
// }
//
// #[derive(Debug)]
// pub struct Client {
//     _handle: ClientHandle,            // handle used to send the client events
//     stream: UnixStream,               // the client owns the stream
//     rx: mpsc::Receiver<TerminalWidgetEvent>,  // receiver for client events
//     daemon_state: DaemonState,        // determines if currently accepting events from daemon
//     sync_daemon_state: bool,          // if the state is dirty only then do we need to sync to the ui
//     ui_stdin_tx: mpsc::Sender<Bytes>, // this is for popup actor to connect to stdin
//     ui_handle: UIHandle,              // how the client sends messages to ui
//     input_parser: InputParser,        // converts streams of bytes into actionable events
//     client_state: ClientWidgetEvent,        // the current state of the client
// }
// impl Client {
//     #[instrument(skip(stream))]
//     pub fn spawn(stream: UnixStream, daemon_state: DaemonState) -> Result<CliTask> {
//         Client::new(stream, daemon_state)?.run()
//     }
//
//     #[instrument(skip(stream))]
//     fn new(stream: UnixStream, daemon_state: DaemonState) -> Result<Self> {
//         let (tx, rx) = mpsc::channel(100);
//         let (ui_stdin_tx, ui_stdin_rx) = mpsc::channel(100);
//         let handle = ClientHandle { tx };
//         let ui_handle = UI::spawn(handle.clone(), ui_stdin_rx)?;
//         Ok(Self {
//             _handle: handle,
//             stream,
//             rx,
//             ui_stdin_tx,
//             ui_handle,
//             daemon_state,
//             sync_daemon_state: true,
//             input_parser: InputParser::new(),
//             client_state: ClientWidgetEvent::Normal,
//         })
//     }
//
//     #[instrument(skip(self), fields(client_state = ?self.client_state))]
//     fn run(mut self) -> Result<CliTask> {
//         let task: CliTask = tokio::spawn({
//             let span = tracing::Span::current();
//             let mut stdin = tokio::io::stdin();
//             let mut stdin_buf = [0u8; 1024];
//             async move {
//                 let mut ticker = interval(Duration::from_millis(1000));
//                 loop {
//                     tokio::select! {
//                         Some(event) = self.rx.recv() => {
//                             match event {
//                                 Selected(index) => {
//                                     debug!("Selected: {index:?}");
//                                     match self.client_state {
//                                         ClientWidgetEvent::Normal => {
//                                             error!("should not receive selected event in normal state");
//                                         },
//                                         ClientWidgetEvent::SelectingSession => {
//                                             if let Some(index) = index {
//                                                 let selected_session = self.daemon_state.session_ids[index];
//                                                 debug!("sending session selection: {selected_session}");
//                                                 comm::send_event(&mut self.stream, CliEvent::SwitchSession(selected_session)).await.unwrap();
//                                             }
//                                             debug!("Returning to normal state");
//                                             self.client_state = ClientWidgetEvent::Normal;
//                                         }
//                                     }
//                                 }
//                             }
//                         },
//                         res = comm::recv_daemon_event(&mut self.stream) => {
//                             match res {
//                                 Ok(event) => {
//                                     match event {
//                                         DaemonEvent::Raw(bytes) => {
//                                             trace!("DaemonEvent(Raw({bytes:?}))");
//                                             self.ui_handle.output(bytes).await?;
//                                         }
//                                         DaemonEvent::Disconnected => {
//                                             debug!("DaemonEvent(Disconnected)");
//                                             self.ui_handle.kill().await.unwrap();
//                                             break;
//                                         }
//                                         DaemonEvent::CurrentSessions(session_ids) => {
//                                             debug!("DaemonEvent(CurrentSessions({session_ids:?}))");
//                                             self.daemon_state.set_sessions(session_ids);
//                                             self.sync_daemon_state = true;
//                                         }
//                                         DaemonEvent::ActiveSession(session_id) => {
//                                             debug!("DaemonEvent(ActiveSession({session_id}))");
//                                             self.daemon_state.set_active_session(session_id);
//                                             self.sync_daemon_state = true;
//                                         }
//                                         DaemonEvent::NewSession(session_id) => {
//                                             debug!("DaemonEvent(NewSession({session_id}))");
//                                             self.daemon_state.add_session(session_id);
//                                             self.sync_daemon_state = true;
//                                         }
//                                         DaemonEvent::DeletedSession(_session_id) => {
//                                             todo!("implement delete session");
//                                         }
//                                     }
//                                 }
//                                 Err(e) => {
//                                     error!("Error receiving daemon event: {e}");
//                                     break;
//                                 }
//                             }
//                         }
//                         stdin_res = stdin.read(&mut stdin_buf) => {
//                             match stdin_res {
//                                 Ok(n) if n > 0 => {
//                                     match self.client_state {
//                                         ClientWidgetEvent::Normal => {
//                                             trace!("Sending {n} bytes to Daemon");
//                                             for event in self.input_parser.process(&stdin_buf[..n]) {
//                                                 match event {
//                                                     ParsedEvent::DaemonAction(cli_event) => {
//                                                         comm::send_event(&mut self.stream, cli_event).await?;
//                                                     },
//                                                     ParsedEvent::LocalAction(local_action) => {
//                                                         match local_action {
//                                                             Action::SwitchSession => {
//                                                                 self.client_state = ClientWidgetEvent::SelectingSession;
//                                                                 let items = DisplayableVec::new(self.daemon_state.session_ids.clone());
//                                                                 self.ui_handle.select_fuzzy(items, "Select Session".to_owned()).await.unwrap();
//                                                             },
//                                                         }
//                                                     },
//                                                 }
//                                             }
//
//                                         },
//                                         ClientWidgetEvent::SelectingSession => {
//                                             trace!("Sending {n} bytes to ui");
//                                             self.ui_stdin_tx.send(Bytes::copy_from_slice(&stdin_buf[..n])).await?;
//                                         },
//                                     }
//                                 }
//                                 Ok(_) => {
//                                     break;
//                                 }
//                                 Err(e) => {
//                                     error!("Error receiving stdin: {e}");
//                                     continue;
//                                 }
//                             }
//                         },
//                         _ = ticker.tick(), if self.sync_daemon_state => {
//                             debug!("syncing daemon_state");
//                             self.ui_handle.sync_daemon_state(self.daemon_state.clone()).await?;
//                             self.sync_daemon_state = false;
//                         }
//                     }
//                 }
//                 debug!("Client stopped");
//                 Ok(())
//             }.instrument(span)
//         });
//
//         Ok(task)
//     }
// }
//
//
