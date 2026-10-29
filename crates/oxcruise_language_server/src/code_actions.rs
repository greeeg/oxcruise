//! Quick fixes that insert the suppression comments `oxcruise_extract` understands.

use std::collections::HashMap;

use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionParams, Diagnostic, NumberOrString,
    Position, Range, TextEdit, Uri, WorkspaceEdit,
};

use crate::diagnostics::SOURCE;
use crate::source::Source;

/// `//` comments are only valid in these; `.vue`/`.svelte`/`.astro` files get none for now.
fn comment_syntax_is_js(uri: &Uri) -> bool {
    let name = uri.as_str().rsplit('/').next().unwrap_or("");
    matches!(
        name.rsplit_once('.').map(|(_, e)| e),
        Some("js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx" | "mts" | "cts")
    )
}

pub fn code_actions(
    uri: &Uri,
    source: &Source,
    params: &CodeActionParams,
) -> Vec<CodeActionOrCommand> {
    if !comment_syntax_is_js(uri) || !source.is_readable() {
        return vec![];
    }
    let mut seen = Vec::new();
    let mut actions = Vec::new();
    for d in &params.context.diagnostics {
        let Some((rule, on_edge)) = ours(d) else {
            continue;
        };
        if on_edge {
            let line = d.range.start.line;
            let indent: String = source
                .line(line)
                .unwrap_or("")
                .chars()
                .take_while(|c| c.is_whitespace())
                .collect();
            let title = format!("Ignore `{rule}` for this line");
            let text = format!("{indent}// oxcruise-ignore-next-line {rule}\n");
            push(&mut actions, &mut seen, uri, d, title, line, text);
        }
        // an `oxcruise-ignore-file` comment only counts before the first statement
        let line = u32::from(source.line(0).is_some_and(|l| l.starts_with("#!")));
        let title = format!("Ignore `{rule}` for this file");
        let text = format!("// oxcruise-ignore-file {rule}\n");
        push(&mut actions, &mut seen, uri, d, title, line, text);
    }
    actions
}

/// The rule of one of our diagnostics, and whether it sits on an import (a line comment reaches it).
fn ours(d: &Diagnostic) -> Option<(&str, bool)> {
    if d.source.as_deref() != Some(SOURCE) {
        return None;
    }
    let Some(NumberOrString::String(rule)) = &d.code else {
        return None;
    };
    let on_edge = matches!(
        d.data.as_ref().and_then(|v| v.as_str()),
        Some("dependency" | "cycle")
    );
    Some((rule, on_edge))
}

fn push(
    actions: &mut Vec<CodeActionOrCommand>,
    seen: &mut Vec<(String, u32)>,
    uri: &Uri,
    diagnostic: &Diagnostic,
    title: String,
    line: u32,
    new_text: String,
) {
    if seen.contains(&(title.clone(), line)) {
        return;
    }
    seen.push((title.clone(), line));
    let at = Position::new(line, 0);
    let edit = TextEdit {
        range: Range::new(at, at),
        new_text,
    };
    actions.push(CodeActionOrCommand::CodeAction(CodeAction {
        title,
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diagnostic.clone()]),
        edit: Some(WorkspaceEdit {
            changes: Some(HashMap::from([(uri.clone(), vec![edit])])),
            ..Default::default()
        }),
        ..Default::default()
    }));
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use lsp_types::CodeActionContext;

    use super::*;

    fn diag(rule: &str, kind: &str, line: u32) -> Diagnostic {
        Diagnostic {
            range: Range::new(Position::new(line, 4), Position::new(line, 9)),
            code: Some(NumberOrString::String(rule.into())),
            source: Some(SOURCE.into()),
            data: Some(serde_json::json!(kind)),
            ..Default::default()
        }
    }

    fn run(file: &str, text: &str, diagnostics: Vec<Diagnostic>) -> Vec<(String, TextEdit)> {
        let uri = Uri::from_str(&format!("file:///p/{file}")).unwrap();
        let params = CodeActionParams {
            text_document: lsp_types::TextDocumentIdentifier { uri: uri.clone() },
            range: Range::default(),
            context: CodeActionContext {
                diagnostics,
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };
        code_actions(&uri, &Source::from_text(text), &params)
            .into_iter()
            .map(|a| {
                let CodeActionOrCommand::CodeAction(a) = a else {
                    unreachable!()
                };
                let edit = a
                    .edit
                    .unwrap()
                    .changes
                    .unwrap()
                    .remove(&uri)
                    .unwrap()
                    .remove(0);
                (a.title, edit)
            })
            .collect()
    }

    #[test]
    fn line_fix_keeps_the_indentation() {
        let actions = run(
            "a.ts",
            "x\n  import './b';\n",
            vec![diag("no-circular", "cycle", 1)],
        );
        assert_eq!(actions[0].0, "Ignore `no-circular` for this line");
        assert_eq!(actions[0].1.range.start, Position::new(1, 0));
        assert_eq!(
            actions[0].1.new_text,
            "  // oxcruise-ignore-next-line no-circular\n"
        );
        assert_eq!(actions[1].0, "Ignore `no-circular` for this file");
        assert_eq!(actions[1].1.range.start, Position::new(0, 0));
    }

    #[test]
    fn module_violations_only_get_the_file_fix() {
        let actions = run("a.ts", "x\n", vec![diag("no-orphans", "module", 0)]);
        assert_eq!(actions.len(), 1);
        assert_eq!(
            actions[0].1.new_text,
            "// oxcruise-ignore-file no-orphans\n"
        );
    }

    #[test]
    fn file_comment_goes_after_a_shebang() {
        let actions = run(
            "a.mjs",
            "#!/usr/bin/env node\nimport 'x';\n",
            vec![diag("r", "module", 0)],
        );
        assert_eq!(actions[0].1.range.start, Position::new(1, 0));
    }

    /// Insert an edit's text (always at column 0) into `text`.
    fn apply(text: &str, edit: &TextEdit) -> String {
        let mut lines: Vec<&str> = text.split_inclusive('\n').collect();
        lines.insert(edit.range.start.line as usize, &edit.new_text);
        lines.concat()
    }

    fn facts(file: &str, text: &str) -> oxcruise_extract::FileFacts {
        let opts = oxcruise_extract::ExtractOptions::default();
        let alloc = oxc_allocator::Allocator::default();
        oxcruise_extract::extract(std::path::Path::new(file), text, &opts, &alloc)
    }

    #[test]
    fn the_comments_are_honoured_by_the_extractor() {
        let text = "#!/usr/bin/env node\nimport a from './a';\n  import b from './b';\n";
        let on_b = diag("no-circular", "dependency", 2);
        let actions = run("a.mjs", text, vec![on_b]);

        let line_fixed = facts("a.mjs", &apply(text, &actions[0].1));
        assert!(line_fixed.deps[0].suppressed.is_empty());
        assert_eq!(line_fixed.deps[1].suppressed, ["no-circular"]);

        let file_fixed = facts("a.mjs", &apply(text, &actions[1].1));
        assert_eq!(file_fixed.file_suppressions.len(), 1);
        assert_eq!(file_fixed.file_suppressions[0].rules, ["no-circular"]);
    }

    #[test]
    fn skips_foreign_diagnostics_duplicates_and_other_languages() {
        let mut other = diag("r", "dependency", 0);
        other.source = Some("eslint".into());
        assert!(run("a.ts", "x\n", vec![other]).is_empty());

        let twice = vec![diag("r", "dependency", 0), diag("r", "dependency", 0)];
        assert_eq!(run("a.ts", "x\n", twice).len(), 2);

        assert!(run("a.vue", "x\n", vec![diag("r", "dependency", 0)]).is_empty());
    }
}
