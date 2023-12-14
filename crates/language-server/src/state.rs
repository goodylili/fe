use std::sync::{Arc, Mutex};

use crate::db::LanguageServerDatabase;
use crate::workspace::Workspace;
use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};
use log::{info, Level, Metadata, Record};
use log::{LevelFilter, SetLoggerError};
use lsp_server::Message;
use lsp_types::notification::Notification;
use lsp_types::request::Request;

use crate::handlers::notifications::handle_document_did_change;
use crate::handlers::request::handle_goto_definition;
use crate::handlers::{notifications::handle_document_did_open, request::handle_hover};

pub struct ServerState {
    pub(crate) sender: Arc<Mutex<Sender<Message>>>,
    pub(crate) db: LanguageServerDatabase,
    pub(crate) workspace: Workspace,
}

impl ServerState {
    pub fn new(sender: Sender<Message>) -> Self {
        let sender = Arc::new(Mutex::new(sender));

        Self {
            sender,
            db: LanguageServerDatabase::default(),
            workspace: Workspace::default(),
        }
    }

    fn send(&mut self, msg: Message) -> Result<()> {
        info!("SEND: {:?}", msg);
        let sender = self.sender.lock().unwrap();
        sender.send(msg)?;
        Ok(())
    }

    pub fn run(&mut self, receiver: Receiver<lsp_server::Message>) -> Result<()> {
        info!("Fe Language Server listening...");

        // watch the workspace root for changes
        self.send(lsp_server::Message::Request(lsp_server::Request::new(
            28_716_283.into(),
            String::from("client/registerCapability"),
            lsp_types::RegistrationParams {
                registrations: vec![lsp_types::Registration {
                    id: String::from("watch-fe-files"),
                    method: String::from("workspace/didChangeWatchedFiles"),
                    register_options: Some(
                        serde_json::to_value(lsp_types::DidChangeWatchedFilesRegistrationOptions {
                            watchers: vec![lsp_types::FileSystemWatcher {
                                glob_pattern: lsp_types::GlobPattern::String("**/*.fe".to_string()),
                                kind: None, // kind: Some(WatchKind::Create | WatchKind::Change | WatchKind::Delete),
                            }],
                        })
                        .unwrap(),
                    ),
                }],
            },
        )))?;

        while let Some(msg) = self.next_message(&receiver) {
            if let lsp_server::Message::Notification(notification) = &msg {
                if notification.method == lsp_types::notification::Exit::METHOD {
                    return Ok(());
                }
            }

            let _ = self.handle_message(msg);
        }
        Ok(())
    }

    fn next_message(&self, receiver: &Receiver<Message>) -> Option<Message> {
        crossbeam_channel::select! {
            recv(receiver) -> msg => msg.ok()
        }
    }

    fn handle_message(&mut self, msg: lsp_server::Message) -> Result<()> {
        if let lsp_server::Message::Request(req) = msg {
            info!("REQUEST: {:?}", req);

            match req.method.as_str() {
                // TODO: implement actually useful hover handler
                lsp_types::request::HoverRequest::METHOD => handle_hover(self, req)?,
                // goto definition
                lsp_types::request::GotoDefinition::METHOD => handle_goto_definition(self, req)?,
                lsp_types::request::GotoTypeDefinition::METHOD => {
                    handle_goto_definition(self, req)?;
                }
                lsp_types::request::GotoImplementation::METHOD => {
                    handle_goto_definition(self, req)?;
                }
                lsp_types::request::GotoDeclaration::METHOD => handle_goto_definition(self, req)?,
                _ => {}
            }
        } else if let lsp_server::Message::Notification(note) = msg {
            // log the notification to the console
            info!("NOTIFICATION: {:?}", note);

            match note.method.as_str() {
                lsp_types::notification::DidOpenTextDocument::METHOD => {
                    handle_document_did_open(self, note)?;
                }
                lsp_types::notification::DidChangeTextDocument::METHOD => {
                    handle_document_did_change(self, note)?;
                }
                // lsp_types::notification::DidChangeWorkspaceFolders::METHOD => {
                //     handle_workspace_did_change_folders(self, note)?
                // }
                _ => {}
            }
        } else if let lsp_server::Message::Response(resp) = msg {
            info!("RESPONSE: {:?}", resp);
        }

        Ok(())
    }

    pub(crate) fn send_response(&mut self, response: lsp_server::Response) -> Result<()> {
        self.send(lsp_server::Message::Response(response))?;
        Ok(())
    }

    pub fn init_logger(&self, level: Level) -> Result<(), SetLoggerError> {
        let logger = LspLogger {
            level,
            sender: self.sender.clone(),
        };
        let static_logger = Box::leak(Box::new(logger));
        log::set_logger(static_logger)?;
        log::set_max_level(LevelFilter::Debug);
        Ok(())
    }
}

pub struct LspLogger {
    level: Level,
    sender: Arc<Mutex<Sender<Message>>>,
}

impl LspLogger {
    fn send(&self, msg: Message) -> Result<()> {
        let sender = self.sender.lock().unwrap();
        sender.send(msg)?;
        Ok(())
    }
}

impl log::Log for LspLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        let logger = self;
        metadata.level() <= logger.level
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let message = format!("{} - {}", record.level(), record.args());
            let _ = self.send(lsp_server::Message::Notification(
                lsp_server::Notification {
                    method: String::from("window/logMessage"),
                    params: serde_json::to_value(lsp_types::LogMessageParams {
                        typ: match record.level() {
                            Level::Error => lsp_types::MessageType::ERROR,
                            Level::Warn => lsp_types::MessageType::WARNING,
                            Level::Info => lsp_types::MessageType::INFO,
                            Level::Debug => lsp_types::MessageType::LOG,
                            Level::Trace => lsp_types::MessageType::LOG,
                        },
                        message,
                    })
                    .unwrap(),
                },
            ));
        }
    }

    fn flush(&self) {}
}
