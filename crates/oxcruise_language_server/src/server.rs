//! The message loop: LSP in, analysis jobs out to a worker thread, diagnostics back.

use std::collections::BTreeMap;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crossbeam_channel::{Sender, select, unbounded};
use lsp_server::{Connection, ErrorCode, Message, Request, Response};
use lsp_types::notification::{
    DidChangeWatchedFiles, DidChangeWorkspaceFolders, DidOpenTextDocument, DidSaveTextDocument,
    Initialized, LogMessage, Notification, PublishDiagnostics, ShowMessage,
};
use lsp_types::request::{CodeActionRequest, HoverRequest, Request as LspRequest};
use lsp_types::{
    CodeActionKind, CodeActionOptions, CodeActionParams, CodeActionProviderCapability, Diagnostic,
    DidChangeWorkspaceFoldersParams, Hover, HoverParams, HoverProviderCapability, InitializeParams,
    LogMessageParams, MessageType, OneOf, PositionEncodingKind, PublishDiagnosticsParams,
    SaveOptions, ServerCapabilities, ShowMessageParams, TextDocumentSyncCapability,
    TextDocumentSyncKind, TextDocumentSyncOptions, TextDocumentSyncSaveOptions, Uri,
    WorkspaceFoldersServerCapabilities, WorkspaceServerCapabilities,
};
use serde::de::DeserializeOwned;

use crate::source::{Encoding, Source};
use crate::uri::{path_to_uri, uri_to_path};
use crate::{AnalyzeFn, AnalyzeOutcome, LspAnalysis, code_actions, diagnostics, hover};

/// Saves in quick succession (save all, a git checkout) run one analysis.
const DEBOUNCE: Duration = Duration::from_millis(200);

struct Job {
    root: PathBuf,
}

struct Finished {
    root: PathBuf,
    outcome: AnalyzeOutcome,
}

struct Root {
    /// As the client named it.
    path: PathBuf,
    /// What file paths in diagnostics are joined onto: `path` unless it only reaches the analysed
    /// directory through a symlink the client does not use.
    base: PathBuf,
    analysis: Option<LspAnalysis>,
    published: BTreeMap<PathBuf, Vec<Diagnostic>>,
    /// When the next analysis should start; set again by every change that arrives before.
    due: Option<Instant>,
    running: bool,
    last_error: Option<String>,
}

impl Root {
    fn new(path: PathBuf) -> Root {
        Root {
            base: path.clone(),
            path,
            analysis: None,
            published: BTreeMap::new(),
            due: Some(Instant::now()),
            running: false,
            last_error: None,
        }
    }
}

struct Server<'a> {
    conn: &'a Connection,
    enc: Encoding,
    roots: Vec<Root>,
    jobs: Sender<Job>,
}

/// Serve one client over `conn` until it asks to exit. Blocks.
pub fn serve(conn: Connection, analyze: AnalyzeFn) -> Result<(), String> {
    let (id, params) = conn.initialize_start().map_err(|e| e.to_string())?;
    let params: InitializeParams = serde_json::from_value(params).map_err(|e| e.to_string())?;
    let utf8 = params
        .capabilities
        .general
        .as_ref()
        .and_then(|g| g.position_encodings.as_ref())
        .is_some_and(|e| e.contains(&PositionEncodingKind::UTF8));
    let enc = if utf8 {
        Encoding::Utf8
    } else {
        Encoding::Utf16
    };
    let result = serde_json::json!({
        "capabilities": capabilities(enc),
        "serverInfo": { "name": "oxcruise", "version": env!("CARGO_PKG_VERSION") },
    });
    conn.initialize_finish(id, result)
        .map_err(|e| e.to_string())?;

    #[allow(deprecated)]
    let folders: Vec<PathBuf> = match params.workspace_folders {
        Some(folders) => folders.iter().filter_map(|f| uri_to_path(&f.uri)).collect(),
        None => params
            .root_uri
            .as_ref()
            .and_then(uri_to_path)
            .into_iter()
            .collect(),
    };

    let (jobs, job_rx) = unbounded::<Job>();
    let (done_tx, done_rx) = unbounded::<Finished>();
    // One worker: runs are CPU-bound and already parallel inside, and it keeps the loop responsive.
    std::thread::Builder::new()
        .name("oxcruise-analyze".into())
        .spawn(move || {
            for job in job_rx {
                let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| analyze(&job.root)))
                    .unwrap_or_else(|_| AnalyzeOutcome {
                        analysis: Err("oxcruise crashed while analysing the project".into()),
                        log: String::new(),
                    });
                if done_tx
                    .send(Finished {
                        root: job.root,
                        outcome,
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .map_err(|e| e.to_string())?;

    let mut server = Server {
        conn: &conn,
        enc,
        roots: folders.into_iter().map(Root::new).collect(),
        jobs,
    };
    loop {
        let wait = server.wait();
        select! {
            recv(conn.receiver) -> msg => match msg {
                Ok(msg) => if server.handle(msg)? { return Ok(()) },
                Err(_) => return Ok(()),
            },
            recv(done_rx) -> done => if let Ok(done) = done { server.finished(done) },
            default(wait) => {}
        }
        server.dispatch_due();
    }
}

fn capabilities(enc: Encoding) -> ServerCapabilities {
    ServerCapabilities {
        position_encoding: Some(enc.kind()),
        text_document_sync: Some(TextDocumentSyncCapability::Options(
            TextDocumentSyncOptions {
                open_close: Some(true),
                change: Some(TextDocumentSyncKind::NONE),
                save: Some(TextDocumentSyncSaveOptions::SaveOptions(SaveOptions {
                    include_text: Some(false),
                })),
                ..Default::default()
            },
        )),
        code_action_provider: Some(CodeActionProviderCapability::Options(CodeActionOptions {
            code_action_kinds: Some(vec![CodeActionKind::QUICKFIX]),
            ..Default::default()
        })),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        workspace: Some(WorkspaceServerCapabilities {
            workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                supported: Some(true),
                change_notifications: Some(OneOf::Left(true)),
            }),
            file_operations: None,
        }),
        ..Default::default()
    }
}

impl Server<'_> {
    /// How long the loop may block before an analysis is due.
    fn wait(&self) -> Duration {
        let now = Instant::now();
        self.roots
            .iter()
            .filter(|r| !r.running)
            .filter_map(|r| r.due)
            .map(|due| due.saturating_duration_since(now))
            .min()
            .unwrap_or(Duration::from_secs(3600))
    }

    fn dispatch_due(&mut self) {
        let now = Instant::now();
        for root in &mut self.roots {
            if !root.running && root.due.is_some_and(|due| due <= now) {
                root.due = None;
                root.running = true;
                let _ = self.jobs.send(Job {
                    root: root.path.clone(),
                });
            }
        }
    }

    /// `true` when the client asked to exit.
    fn handle(&mut self, msg: Message) -> Result<bool, String> {
        match msg {
            Message::Request(req) => {
                if self.conn.handle_shutdown(&req).map_err(|e| e.to_string())? {
                    return Ok(true);
                }
                self.request(req);
            }
            Message::Notification(n) => return Ok(self.notification(n)),
            Message::Response(_) => {}
        }
        Ok(false)
    }

    fn request(&self, req: Request) {
        match req.method.as_str() {
            CodeActionRequest::METHOD => self.reply::<CodeActionRequest>(req, Self::code_action),
            HoverRequest::METHOD => self.reply::<HoverRequest>(req, Self::hover),
            _ => self.send(Message::Response(Response::new_err(
                req.id,
                ErrorCode::MethodNotFound as i32,
                format!("unsupported request {}", req.method),
            ))),
        }
    }

    fn reply<R: LspRequest>(&self, req: Request, handler: fn(&Self, R::Params) -> R::Result) {
        let response = match serde_json::from_value::<R::Params>(req.params) {
            Ok(params) => Response::new_ok(req.id, handler(self, params)),
            Err(e) => Response::new_err(req.id, ErrorCode::InvalidParams as i32, e.to_string()),
        };
        self.send(Message::Response(response));
    }

    fn notification(&mut self, n: lsp_server::Notification) -> bool {
        fn params<N: Notification>(n: lsp_server::Notification) -> Option<N::Params>
        where
            N::Params: DeserializeOwned,
        {
            serde_json::from_value(n.params).ok()
        }
        match n.method.as_str() {
            "exit" => return true,
            Initialized::METHOD => {
                for root in &mut self.roots {
                    root.due = Some(Instant::now());
                }
            }
            DidSaveTextDocument::METHOD => {
                if let Some(p) = params::<DidSaveTextDocument>(n) {
                    self.touch(&p.text_document.uri);
                }
            }
            DidChangeWatchedFiles::METHOD => {
                if let Some(p) = params::<DidChangeWatchedFiles>(n) {
                    for change in p.changes {
                        self.touch(&change.uri);
                    }
                }
            }
            DidOpenTextDocument::METHOD => {
                if let Some(p) = params::<DidOpenTextDocument>(n)
                    && let Some(i) = self.root_of(&p.text_document.uri)
                {
                    let root = &mut self.roots[i];
                    if root.analysis.is_none()
                        && !root.running
                        && root.due.is_none()
                        && root.last_error.is_none()
                    {
                        root.due = Some(Instant::now());
                    }
                }
            }
            DidChangeWorkspaceFolders::METHOD => {
                if let Some(p) = params::<DidChangeWorkspaceFolders>(n) {
                    self.folders_changed(p);
                }
            }
            _ => {}
        }
        false
    }

    /// A file changed on disk: its root needs a new analysis soon.
    fn touch(&mut self, uri: &Uri) {
        if let Some(i) = self.root_of(uri) {
            self.roots[i].due = Some(Instant::now() + DEBOUNCE);
        }
    }

    fn folders_changed(&mut self, p: DidChangeWorkspaceFoldersParams) {
        for removed in p.event.removed {
            let Some(path) = uri_to_path(&removed.uri) else {
                continue;
            };
            if let Some(i) = self.roots.iter().position(|r| r.path == path) {
                let root = self.roots.remove(i);
                for file in root.published.keys() {
                    self.publish(file, vec![]);
                }
            }
        }
        for added in p.event.added {
            if let Some(path) = uri_to_path(&added.uri)
                && !self.roots.iter().any(|r| r.path == path)
            {
                self.roots.push(Root::new(path));
            }
        }
    }

    /// The root whose directory holds `uri` (the innermost one for nested roots).
    fn root_of(&self, uri: &Uri) -> Option<usize> {
        let path = uri_to_path(uri)?;
        self.roots
            .iter()
            .enumerate()
            .filter(|(_, r)| path.starts_with(&r.base) || path.starts_with(&r.path))
            .max_by_key(|(_, r)| r.base.components().count())
            .map(|(i, _)| i)
    }

    /// Root, absolute path and root-relative posix path of a document.
    fn locate(&self, uri: &Uri) -> Option<(&Root, PathBuf, String)> {
        let path = uri_to_path(uri)?;
        let root = &self.roots[self.root_of(uri)?];
        let rel = path.strip_prefix(&root.base).ok()?;
        let rel = rel.to_str()?.replace('\\', "/");
        Some((root, path, rel))
    }

    fn code_action(&self, params: CodeActionParams) -> Option<Vec<lsp_types::CodeActionOrCommand>> {
        let uri = &params.text_document.uri;
        let (_, path, _) = self.locate(uri)?;
        let actions = code_actions::code_actions(uri, &Source::read(&path), &params);
        Some(actions)
    }

    fn hover(&self, params: HoverParams) -> Option<Hover> {
        let at = params.text_document_position_params;
        let (root, path, rel) = self.locate(&at.text_document.uri)?;
        hover::hover(
            root.analysis.as_ref()?,
            &rel,
            &Source::read(&path),
            at.position,
            self.enc,
        )
    }

    fn finished(&mut self, done: Finished) {
        let Some(i) = self.roots.iter().position(|r| r.path == done.root) else {
            return;
        };
        self.roots[i].running = false;
        if !done.outcome.log.trim().is_empty() {
            self.log(MessageType::WARNING, done.outcome.log.trim().to_string());
        }
        // Something changed while this ran: its result is already out of date.
        if self.roots[i].due.is_some() {
            return;
        }
        match done.outcome.analysis {
            Ok(analysis) => {
                let root = &mut self.roots[i];
                root.last_error = None;
                root.base = if same_dir(&root.path, &analysis.cwd) {
                    root.path.clone()
                } else {
                    analysis.cwd.clone()
                };
                let next = diagnostics::by_file(&root.base, &analysis.violations, self.enc);
                let previous = std::mem::replace(&mut root.published, next);
                root.analysis = Some(analysis);

                let root = &self.roots[i];
                for (file, diagnostics) in &root.published {
                    if previous.get(file) != Some(diagnostics) {
                        self.publish(file, diagnostics.clone());
                    }
                }
                for file in previous.keys().filter(|f| !root.published.contains_key(*f)) {
                    self.publish(file, vec![]);
                }
            }
            Err(message) => {
                let root = &mut self.roots[i];
                root.analysis = None;
                let previous = std::mem::take(&mut root.published);
                let first_time = root.last_error.as_deref() != Some(&message);
                root.last_error = Some(message.clone());
                for file in previous.keys() {
                    self.publish(file, vec![]);
                }
                if first_time {
                    self.notify::<ShowMessage>(ShowMessageParams {
                        typ: MessageType::ERROR,
                        message: format!("oxcruise: {message}"),
                    });
                }
            }
        }
    }

    fn publish(&self, file: &Path, diagnostics: Vec<Diagnostic>) {
        if let Some(uri) = path_to_uri(file) {
            self.notify::<PublishDiagnostics>(PublishDiagnosticsParams {
                uri,
                diagnostics,
                version: None,
            });
        }
    }

    fn log(&self, typ: MessageType, message: String) {
        self.notify::<LogMessage>(LogMessageParams { typ, message });
    }

    fn notify<N: Notification>(&self, params: N::Params) {
        self.send(Message::Notification(lsp_server::Notification::new(
            N::METHOD.to_string(),
            params,
        )));
    }

    fn send(&self, msg: Message) {
        // a closed connection ends the loop on its own
        let _ = self.conn.sender.send(msg);
    }
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}
