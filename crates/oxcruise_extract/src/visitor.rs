use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_span::GetSpan;
use oxc_syntax::module_record::{
    ExportExportName, ExportImportName, ImportImportName, ModuleRecord,
};
use oxcruise_core::{DepKinds, ModuleSystem};
use rustc_hash::FxHashMap;

use crate::facts::{ExportFact, ExportKind, FactsBuilder, ImportFact};
use crate::shift;

/// ES module imports and exports, taken from oxc's `ModuleRecord`.
pub(crate) fn collect_module_record(
    record: &ModuleRecord<'_>,
    offset: u32,
    b: &mut FactsBuilder<'_>,
) {
    b.has_module_syntax |= record.has_module_syntax;

    // statement start -> (entries, type-only entries). A statement whose every
    // specifier is `type` (`import { type A, type B } from "x"`) is type-only
    // even though the statement itself has no `type` keyword.
    let mut stmt_entries: FxHashMap<u32, (u32, u32)> = FxHashMap::default();
    for e in &record.import_entries {
        let c = stmt_entries.entry(e.statement_span.start).or_default();
        c.0 += 1;
        c.1 += u32::from(e.is_type);
    }
    for e in record
        .indirect_export_entries
        .iter()
        .chain(record.star_export_entries.iter())
    {
        if e.module_request.is_some() {
            let c = stmt_entries.entry(e.statement_span.start).or_default();
            c.0 += 1;
            c.1 += u32::from(e.is_type);
        }
    }

    for (specifier, occurrences) in &record.requested_modules {
        for occ in occurrences.iter() {
            let all_specifiers_type = stmt_entries
                .get(&occ.statement_span.start)
                .is_some_and(|&(n, t)| n > 0 && n == t);
            let mut kinds = if occ.is_import {
                DepKinds::IMPORT
            } else {
                DepKinds::EXPORT
            };
            if occ.is_type || all_specifiers_type {
                kinds |= DepKinds::TYPE_ONLY;
            }
            b.push_dep(
                specifier.as_str(),
                kinds,
                ModuleSystem::Es6,
                shift(occ.span, offset),
                occ.statement_span.start + offset,
            );
        }
    }

    for e in &record.import_entries {
        let imported = match &e.import_name {
            ImportImportName::Name(n) => n.name.to_string(),
            ImportImportName::NamespaceObject => "*".to_string(),
            ImportImportName::Default(_) => "default".to_string(),
        };
        b.imports.push(ImportFact {
            specifier: e.module_request.name.to_string(),
            imported,
            local: e.local_name.name.to_string(),
            is_type: e.is_type,
        });
    }

    let export_name = |e: &oxc_syntax::module_record::ExportEntry<'_>| match &e.export_name {
        ExportExportName::Name(n) => n.name.to_string(),
        ExportExportName::Default(_) => "default".to_string(),
        ExportExportName::Null => "*".to_string(),
    };
    for e in &record.local_export_entries {
        b.exports.push(ExportFact {
            name: export_name(e),
            kind: ExportKind::Local,
            from: None,
            is_type: e.is_type,
        });
    }
    for e in &record.indirect_export_entries {
        let kind = match e.import_name {
            ExportImportName::All | ExportImportName::AllButDefault => ExportKind::Namespace,
            _ => ExportKind::Reexport,
        };
        b.exports.push(ExportFact {
            name: export_name(e),
            kind,
            from: e.module_request.as_ref().map(|m| m.name.to_string()),
            is_type: e.is_type,
        });
    }
    for e in &record.star_export_entries {
        b.exports.push(ExportFact {
            name: "*".to_string(),
            kind: ExportKind::Star,
            from: e.module_request.as_ref().map(|m| m.name.to_string()),
            is_type: e.is_type,
        });
    }
}

/// Everything that is not a static import/export: dynamic `import()`,
/// `require`, `import x = require()`, `import("x")` types, AMD, URLs, configured calls.
pub(crate) fn collect_ast(program: &Program<'_>, offset: u32, b: &mut FactsBuilder<'_>) {
    let mut v = AstVisitor {
        b,
        offset,
        stmt_start: None,
    };
    v.visit_program(program);
}

struct AstVisitor<'b, 's> {
    b: &'b mut FactsBuilder<'s>,
    offset: u32,
    /// Start of the innermost enclosing statement, if any.
    stmt_start: Option<u32>,
}

impl AstVisitor<'_, '_> {
    fn push(&mut self, spec: &str, kinds: DepKinds, ms: ModuleSystem, span: oxc_span::Span) {
        let stmt = self.stmt_start.unwrap_or(span.start);
        self.b.push_dep(
            spec,
            kinds,
            ms,
            shift(span, self.offset),
            stmt + self.offset,
        );
    }
}

fn static_string<'a>(e: &'a Expression<'_>) -> Option<&'a str> {
    match e.without_parentheses() {
        Expression::StringLiteral(s) => s.value.as_str(),
        Expression::TemplateLiteral(t) if t.expressions.is_empty() && t.quasis.len() == 1 => {
            t.quasis[0].value.cooked.as_ref().and_then(|c| c.as_str())
        }
        _ => None,
    }
}

fn callee_name(e: &Expression<'_>) -> Option<String> {
    match e.without_parentheses() {
        Expression::Identifier(i) => Some(i.name.to_string()),
        Expression::StaticMemberExpression(m) => {
            callee_name(&m.object).map(|o| format!("{o}.{}", m.property.name))
        }
        _ => None,
    }
}

fn is_import_meta_url(e: &Expression<'_>) -> bool {
    match e.without_parentheses() {
        Expression::StaticMemberExpression(m) => {
            m.property.name == "url"
                && matches!(m.object.without_parentheses(), Expression::ImportMeta(_))
        }
        _ => false,
    }
}

/// `new URL("./x", import.meta.url)` -> `"./x"`
fn url_target<'a>(ne: &'a NewExpression<'_>) -> Option<&'a str> {
    if !matches!(&ne.callee, Expression::Identifier(i) if i.name == "URL")
        || ne.arguments.len() != 2
    {
        return None;
    }
    let first = ne.arguments[0].as_expression()?;
    let second = ne.arguments[1].as_expression()?;
    if !is_import_meta_url(second) {
        return None;
    }
    static_string(first)
}

fn relative(spec: &str) -> String {
    if spec.starts_with('.') || spec.starts_with('/') || spec.contains(':') {
        spec.to_string()
    } else {
        format!("./{spec}")
    }
}

impl<'a> Visit<'a> for AstVisitor<'_, '_> {
    fn visit_statement(&mut self, it: &Statement<'a>) {
        let prev = self.stmt_start;
        self.stmt_start = Some(it.span().start);
        walk::walk_statement(self, it);
        self.stmt_start = prev;
    }

    fn visit_import_expression(&mut self, it: &ImportExpression<'a>) {
        if let Some(s) = static_string(&it.source) {
            self.push(
                s,
                DepKinds::DYNAMIC_IMPORT,
                ModuleSystem::Es6,
                it.source.span(),
            );
        }
        walk::walk_import_expression(self, it);
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        if let Some(lit) = it.common_js_require() {
            if let Some(v) = lit.value.as_str() {
                self.push(v, DepKinds::REQUIRE, ModuleSystem::Cjs, lit.span);
            }
        } else if let Some(name) = callee_name(&it.callee) {
            let first = it.arguments.first().and_then(|a| a.as_expression());
            if let Some(spec) = first.and_then(static_string) {
                let span = first.map_or(it.span, |e| e.span());
                let opts = self.b.opts;
                if opts.exotic_require_strings.contains(&name) {
                    self.push(
                        spec,
                        DepKinds::EXOTIC_REQUIRE | DepKinds::REQUIRE,
                        ModuleSystem::Cjs,
                        span,
                    );
                } else if opts.calls.contains(&name) {
                    let ms = if name.starts_with("require") {
                        ModuleSystem::Cjs
                    } else {
                        ModuleSystem::Es6
                    };
                    self.push(spec, DepKinds::CALL, ms, span);
                }
            }
            // AMD: define([...], fn) / define("name", [...], fn) / require([...], fn)
            if !self.b.has_module_syntax && (name == "define" || name == "require") {
                let arr = it.arguments.iter().find_map(|a| {
                    match a.as_expression()?.without_parentheses() {
                        Expression::ArrayExpression(arr) => Some(arr),
                        _ => None,
                    }
                });
                if let Some(arr) = arr {
                    let kind = if name == "define" {
                        DepKinds::AMD_DEFINE
                    } else {
                        DepKinds::AMD_REQUIRE
                    };
                    for el in &arr.elements {
                        if let Some(e) = el.as_expression()
                            && let Some(s) = static_string(e)
                        {
                            // `require`, `exports` and `module` are AMD pseudo-dependencies.
                            if !matches!(s, "require" | "exports" | "module") {
                                self.push(s, kind, ModuleSystem::Amd, e.span());
                            }
                        }
                    }
                }
            }
        }
        walk::walk_call_expression(self, it);
    }

    fn visit_new_expression(&mut self, it: &NewExpression<'a>) {
        if let Some(t) = url_target(it) {
            let span = it.arguments[0].span();
            self.push(&relative(t), DepKinds::URL, ModuleSystem::Es6, span);
        } else if matches!(&it.callee, Expression::Identifier(i) if i.name == "Worker" || i.name == "SharedWorker")
            && let Some(Expression::NewExpression(inner)) =
                it.arguments.first().and_then(|a| a.as_expression())
            && let Some(t) = url_target(inner)
        {
            let span = inner.arguments[0].span();
            self.push(
                &relative(t),
                DepKinds::URL | DepKinds::WORKER,
                ModuleSystem::Es6,
                span,
            );
        }
        walk::walk_new_expression(self, it);
    }

    fn visit_ts_import_equals_declaration(&mut self, it: &TSImportEqualsDeclaration<'a>) {
        if let TSModuleReference::ExternalModuleReference(r) = &it.module_reference {
            let mut kinds = DepKinds::IMPORT_EQUALS;
            if it.import_kind.is_type() {
                kinds |= DepKinds::TYPE_ONLY;
            }
            if let Some(v) = r.expression.value.as_str() {
                self.push(v, kinds, ModuleSystem::Cjs, r.expression.span);
            }
        }
        walk::walk_ts_import_equals_declaration(self, it);
    }

    fn visit_ts_import_type(&mut self, it: &TSImportType<'a>) {
        if let Some(v) = it.source.value.as_str() {
            self.push(
                v,
                DepKinds::TYPE_IMPORT | DepKinds::TYPE_ONLY,
                ModuleSystem::Es6,
                it.source.span,
            );
        }
        walk::walk_ts_import_type(self, it);
    }
}
