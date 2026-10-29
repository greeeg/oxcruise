//! The language server end to end: `oxcruise --lsp` over an in-memory connection, analysing a
//! project on disk with the real config loader, graph and rules.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use lsp_server::{Connection, Message, Notification, Request, RequestId};
use oxcruise::CliRunner;
use serde_json::{Value, json};

struct Client {
    conn: Connection,
    next_id: i32,
    /// latest `publishDiagnostics` per uri
    diagnostics: BTreeMap<String, Vec<Value>>,
    shown: Vec<String>,
}

impl Client {
    fn start(args: &[&str]) -> (Client, std::thread::JoinHandle<()>) {
        let (client, server) = Connection::memory();
        let args = args.iter().map(|s| s.to_string()).collect();
        let handle = std::thread::spawn(move || {
            CliRunner::new(args).serve_lsp_on(server);
        });
        let client = Client {
            conn: client,
            next_id: 0,
            diagnostics: BTreeMap::new(),
            shown: vec![],
        };
        (client, handle)
    }

    fn notify(&self, method: &str, params: Value) {
        let n = Notification::new(method.into(), params);
        self.conn.sender.send(Message::Notification(n)).unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = RequestId::from(self.next_id);
        let r = Request::new(id.clone(), method.into(), params);
        self.conn.sender.send(Message::Request(r)).unwrap();
        loop {
            match self.recv() {
                Some(Message::Response(r)) if r.id == id => {
                    assert!(r.error.is_none(), "{method}: {:?}", r.error);
                    return r.result.unwrap_or(Value::Null);
                }
                Some(_) => {}
                None => panic!("no response to {method}"),
            }
        }
    }

    /// Next message within a few seconds, with diagnostics and messages recorded on the way.
    fn recv(&mut self) -> Option<Message> {
        let msg = self
            .conn
            .receiver
            .recv_timeout(Duration::from_secs(20))
            .ok()?;
        if let Message::Notification(n) = &msg {
            match n.method.as_str() {
                "textDocument/publishDiagnostics" => {
                    let uri = n.params["uri"].as_str().unwrap().to_string();
                    let list = n.params["diagnostics"].as_array().unwrap().clone();
                    self.diagnostics.insert(uri, list);
                }
                "window/showMessage" => {
                    self.shown
                        .push(n.params["message"].as_str().unwrap().to_string());
                }
                _ => {}
            }
        }
        Some(msg)
    }

    /// Pump messages until `done` holds.
    fn wait_until(&mut self, what: &str, done: impl Fn(&Client) -> bool) {
        while !done(self) {
            assert!(
                self.recv().is_some(),
                "timed out waiting for {what}; diagnostics so far: {:?}",
                self.diagnostics
            );
        }
    }

    fn initialize(&mut self, root: &Path, position_encodings: Value) {
        let uri = uri(root);
        self.request(
            "initialize",
            json!({
                "processId": null,
                "capabilities": { "general": { "positionEncodings": position_encodings } },
                "workspaceFolders": [{ "uri": uri, "name": "p" }],
            }),
        );
        self.notify("initialized", json!({}));
    }

    fn shutdown(mut self, server: std::thread::JoinHandle<()>) {
        self.request("shutdown", Value::Null);
        self.notify("exit", Value::Null);
        server.join().unwrap();
    }
}

fn uri(path: &Path) -> String {
    format!("file://{}", path.display())
}

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for (p, c) in files {
        let path = d.path().join(p);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, c).unwrap();
    }
    d
}

const CONFIG: &str = r#"{
  "entries": ["src"],
  "forbid": [{ "name": "ui-not-db", "message": "ui must not import db", "from": "src/ui/**", "to": "src/db/**" }]
}"#;

fn cyclic_project() -> tempfile::TempDir {
    project(&[
        (".oxcruiserc.json", CONFIG),
        ("src/a.ts", "import './b';\nexport const a = 1;\n"),
        ("src/b.ts", "import './a';\nexport const b = 1;\n"),
        ("src/ui/page.ts", "  import { s } from '../db/store';\n"),
        ("src/db/store.ts", "export const s = 1;\n"),
    ])
}

#[cfg(unix)]
#[test]
fn diagnostics_follow_saves() {
    let dir = cyclic_project();
    let root = dir.path();
    let (mut c, server) = Client::start(&["--lsp"]);
    c.initialize(root, json!(["utf-8"]));

    let a = uri(&root.join("src/a.ts"));
    let b = uri(&root.join("src/b.ts"));
    let page = uri(&root.join("src/ui/page.ts"));
    // a cycle is reported once, on its first module
    c.wait_until("the first diagnostics", |c| {
        [&a, &page]
            .iter()
            .all(|u| c.diagnostics.get(*u).is_some_and(|d| !d.is_empty()))
    });

    // the cycle: on the import, with a related entry per hop
    let d = &c.diagnostics[&a][0];
    assert_eq!(d["code"], "no-circular");
    assert_eq!(d["source"], "oxcruise");
    assert_eq!(d["severity"], 1);
    assert_eq!(
        d["range"],
        json!({"start": {"line": 0, "character": 7}, "end": {"line": 0, "character": 12}})
    );
    let related = d["relatedInformation"].as_array().unwrap();
    assert_eq!(related.len(), 2);
    assert_eq!(related[0]["message"], "src/a.ts imports src/b.ts");
    assert_eq!(related[1]["location"]["uri"], b);

    // the forbidden import
    let d = &c.diagnostics[&page][0];
    assert_eq!(d["code"], "ui-not-db");
    assert_eq!(d["message"], "ui must not import db");
    assert!(
        d.get("codeDescription").is_none(),
        "custom rules have no docs page"
    );
    assert_eq!(d["range"]["start"], json!({"line": 0, "character": 20}));

    // hover on the specifier names the target and the rule
    let hover = c.request(
        "textDocument/hover",
        json!({ "textDocument": {"uri": page}, "position": {"line": 0, "character": 26} }),
    );
    let text = hover["contents"]["value"].as_str().unwrap();
    assert!(
        text.contains("`../db/store` resolves to `src/db/store.ts` (source)"),
        "{text}"
    );
    assert!(text.contains("ui-not-db"), "{text}");
    let nowhere = c.request(
        "textDocument/hover",
        json!({ "textDocument": {"uri": page}, "position": {"line": 0, "character": 3} }),
    );
    assert!(nowhere.is_null());

    // quick fixes for the diagnostic under the cursor
    let diagnostic = c.diagnostics[&page][0].clone();
    let actions = c.request(
        "textDocument/codeAction",
        json!({
            "textDocument": {"uri": page},
            "range": diagnostic["range"],
            "context": { "diagnostics": [diagnostic] },
        }),
    );
    let edit = &actions[0]["edit"]["changes"][&page][0];
    assert_eq!(actions[0]["title"], "Ignore `ui-not-db` for this line");
    assert_eq!(
        edit["newText"],
        "  // oxcruise-ignore-next-line ui-not-db\n"
    );

    // breaking the cycle on disk and saving clears it
    std::fs::write(root.join("src/b.ts"), "export const b = 1;\n").unwrap();
    c.notify(
        "textDocument/didSave",
        json!({ "textDocument": {"uri": b} }),
    );
    c.wait_until("the cycle to clear", |c| c.diagnostics[&a].is_empty());
    assert!(
        !c.diagnostics[&page].is_empty(),
        "unrelated diagnostics stay"
    );

    // applying the quick fix and saving clears the page's
    std::fs::write(
        root.join("src/ui/page.ts"),
        "  // oxcruise-ignore-next-line ui-not-db\n  import { s } from '../db/store';\n",
    )
    .unwrap();
    c.notify(
        "textDocument/didSave",
        json!({ "textDocument": {"uri": page} }),
    );
    c.wait_until("the suppression to count", |c| {
        c.diagnostics[&page].is_empty()
    });

    c.shutdown(server);
}

#[cfg(unix)]
#[test]
fn utf16_columns_without_utf8_support() {
    let dir = project(&[
        (".oxcruiserc.json", CONFIG),
        ("src/ui/page.ts", "const é = '😀'; import '../db/store';\n"),
        ("src/db/store.ts", "export {};\n"),
    ]);
    let (mut c, server) = Client::start(&["--lsp"]);
    c.initialize(dir.path(), json!(["utf-16"]));
    let page = uri(&dir.path().join("src/ui/page.ts"));
    c.wait_until("diagnostics", |c| c.diagnostics.contains_key(&page));
    // `const é = '😀'; import ` is 23 units but 27 bytes
    assert_eq!(c.diagnostics[&page][0]["range"]["start"]["character"], 23);
    c.shutdown(server);
}

#[cfg(unix)]
#[test]
fn a_broken_config_is_shown_once_and_fixed_by_a_save() {
    let dir = project(&[
        (".oxcruiserc.json", "{ \"entries\": 12 }"),
        ("src/a.ts", "import './b';\n"),
        ("src/b.ts", "import './a';\n"),
    ]);
    let (mut c, server) = Client::start(&["--lsp"]);
    c.initialize(dir.path(), json!(["utf-8"]));
    c.wait_until("the error", |c| !c.shown.is_empty());
    assert!(c.shown[0].starts_with("oxcruise: "), "{:?}", c.shown);

    // the same failure again is not shown again; the fix publishes the diagnostics
    let config = uri(&dir.path().join(".oxcruiserc.json"));
    c.notify(
        "workspace/didChangeWatchedFiles",
        json!({ "changes": [{"uri": config, "type": 2}] }),
    );
    std::fs::write(dir.path().join(".oxcruiserc.json"), "{}").unwrap();
    c.notify(
        "workspace/didChangeWatchedFiles",
        json!({ "changes": [{"uri": config, "type": 2}] }),
    );
    let a = uri(&dir.path().join("src/a.ts"));
    c.wait_until("diagnostics", |c| {
        c.diagnostics.get(&a).is_some_and(|d| !d.is_empty())
    });
    assert_eq!(c.shown.len(), 1, "{:?}", c.shown);
    c.shutdown(server);
}
