use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_span::GetSpan;
use oxc_syntax::module_record::{
    ExportExportName, ExportImportName, ExportLocalName, ImportImportName, ModuleRecord,
};
use oxcruise_core::{DepKinds, ExportMember, ModuleSystem};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::facts::{ExportFact, ExportKind, FactsBuilder, ImportFact, UsedRefs};
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
            // the AST walk narrows this when it runs
            refs: if b.opts.symbols {
                UsedRefs::Members(vec![])
            } else {
                UsedRefs::Opaque
            },
        });
    }

    type Entry<'x> = oxc_syntax::module_record::ExportEntry<'x>;
    let export_name = |e: &Entry<'_>| match &e.export_name {
        ExportExportName::Name(n) => n.name.to_string(),
        ExportExportName::Default(_) => "default".to_string(),
        ExportExportName::Null => "*".to_string(),
    };
    let symbols = b.opts.symbols;
    // `import d from "x"; export { d }` names the local binding where the imported name belongs
    let default_imports: FxHashSet<(u32, &str)> = record
        .import_entries
        .iter()
        .filter(|e| matches!(e.import_name, ImportImportName::Default(_)))
        .map(|e| (e.statement_span.start, e.local_name.name.as_str()))
        .collect();
    let push = |b: &mut FactsBuilder<'_>, mut f: ExportFact, e: &Entry<'_>| {
        if symbols {
            let at = match &e.export_name {
                ExportExportName::Name(n) => n.span,
                ExportExportName::Default(span) => *span,
                ExportExportName::Null => e.span,
            };
            f.is_type |= e.is_type;
            b.export_pos
                .push((at.start + offset, e.statement_span.start + offset));
        }
        b.exports.push(f);
    };
    for e in &record.local_export_entries {
        let mut f = ExportFact::new(export_name(e), ExportKind::Local, None, e.is_type);
        if symbols {
            f.local = match &e.local_name {
                ExportLocalName::Name(n) | ExportLocalName::Default(n) => Some(n.name.to_string()),
                ExportLocalName::Null => None,
            };
        }
        push(b, f, e);
    }
    for e in &record.indirect_export_entries {
        let kind = match e.import_name {
            ExportImportName::All | ExportImportName::AllButDefault => ExportKind::Namespace,
            _ => ExportKind::Reexport,
        };
        let from = e.module_request.as_ref().map(|m| m.name.to_string());
        let mut f = ExportFact::new(export_name(e), kind, from, e.is_type);
        if symbols && let ExportImportName::Name(n) = &e.import_name {
            f.imported = Some(
                if default_imports.contains(&(e.statement_span.start, n.name.as_str())) {
                    "default".to_string()
                } else {
                    n.name.to_string()
                },
            );
        }
        push(b, f, e);
    }
    for e in &record.star_export_entries {
        let from = e.module_request.as_ref().map(|m| m.name.to_string());
        push(
            b,
            ExportFact::new("*".to_string(), ExportKind::Star, from, e.is_type),
            e,
        );
    }
}

/// Everything that is not a static import/export: dynamic `import()`,
/// `require`, `import x = require()`, `import("x")` types, AMD, URLs, configured calls.
pub(crate) fn collect_ast(
    program: &Program<'_>,
    offset: u32,
    first: BlockStart,
    b: &mut FactsBuilder<'_>,
) {
    let symbols = b.opts.symbols.then(|| Symbols::new(first, b));
    let mut v = AstVisitor {
        b,
        offset,
        stmt_start: None,
        symbols,
    };
    v.visit_program(program);
    if let Some(sym) = v.symbols.take() {
        sym.apply(program, offset, b);
    }
}

/// Where the imports and exports of the block being parsed begin in the builder: single-file
/// components have several blocks, and what earlier blocks collected must stay untouched.
#[derive(Clone, Copy)]
pub(crate) struct BlockStart {
    pub imports: usize,
    pub exports: usize,
}

/// What the AST walk learns about bindings, for `ExtractOptions::symbols`. There is no scope
/// analysis: a shadowing local can only make a binding look more used, never less.
struct Symbols {
    first: BlockStart,
    /// import local name -> index in `FactsBuilder::imports`
    import_idx: FxHashMap<String, usize>,
    /// Imports read as `binding.member` only: the members seen.
    members: FxHashMap<usize, Vec<String>>,
    /// Imports used in any other way.
    opaque: FxHashSet<usize>,
    /// Exported local names referenced somewhere in the file.
    referenced: FxHashSet<String>,
    export_locals: FxHashSet<String>,
    /// `export ` keyword of `export <declaration>`, by statement start.
    keyword_fixes: FxHashMap<u32, (u32, u32)>,
    /// Specifier or statement of a listed export, by exported name; `None` when two exports
    /// of the file share the name, which makes the match unsafe.
    listed_fixes: FxHashMap<String, Option<(u32, u32)>>,
}

impl Symbols {
    fn new(first: BlockStart, b: &FactsBuilder<'_>) -> Self {
        Self {
            first,
            import_idx: b
                .imports
                .iter()
                .enumerate()
                .skip(first.imports)
                .map(|(i, f)| (f.local.clone(), i))
                .collect(),
            members: FxHashMap::default(),
            opaque: FxHashSet::default(),
            referenced: FxHashSet::default(),
            export_locals: b.exports[first.exports..]
                .iter()
                .filter_map(|e| e.local.clone())
                .collect(),
            keyword_fixes: FxHashMap::default(),
            listed_fixes: FxHashMap::default(),
        }
    }

    /// Write what was learned into the facts.
    fn apply(self, program: &Program<'_>, offset: u32, b: &mut FactsBuilder<'_>) {
        let BlockStart { imports, exports } = self.first;
        for (i, f) in b.imports.iter_mut().enumerate().skip(imports) {
            if self.opaque.contains(&i) {
                f.refs = UsedRefs::Opaque;
            } else if let Some(m) = self.members.get(&i)
                && let UsedRefs::Members(list) = &mut f.refs
            {
                list.extend(m.iter().cloned());
                list.sort();
                list.dedup();
            }
        }
        let mut per_statement: FxHashMap<u32, usize> = FxHashMap::default();
        for &(_, stmt) in &b.export_pos[exports..] {
            *per_statement.entry(stmt).or_default() += 1;
        }
        for (e, &(_, stmt)) in b.exports[exports..]
            .iter_mut()
            .zip(&b.export_pos[exports..])
        {
            // removing `export` from a statement that exports several names would hide the others
            e.fix = match self.keyword_fixes.get(&(stmt - offset)) {
                Some(&(s, end)) if per_statement[&stmt] == 1 && e.kind == ExportKind::Local => {
                    Some((s + offset, end + offset))
                }
                _ => self
                    .listed_fixes
                    .get(&e.name)
                    .copied()
                    .flatten()
                    .map(|(s, end)| (s + offset, end + offset)),
            };
        }
        let mut members = declared_members(program, offset, b);
        for e in &mut b.exports[exports..] {
            if let Some(local) = &e.local {
                e.used_in_file |= self.referenced.contains(local);
                if let Some(m) = members.remove(local) {
                    e.members = m;
                }
            }
        }
    }
}

/// Members of top-level enums and namespaces by the name they are declared with.
fn declared_members(
    program: &Program<'_>,
    offset: u32,
    b: &FactsBuilder<'_>,
) -> FxHashMap<String, Vec<ExportMember>> {
    let li = b.line_index;
    let member = |name: &str, start: u32| ExportMember {
        name: name.to_string(),
        loc: li.loc(start + offset),
    };
    let mut out: FxHashMap<String, Vec<ExportMember>> = FxHashMap::default();
    let enum_members = |e: &TSEnumDeclaration<'_>| -> Vec<ExportMember> {
        e.body
            .members
            .iter()
            .filter_map(|m| match &m.id {
                TSEnumMemberName::Identifier(i) => Some(member(&i.name, m.span.start)),
                TSEnumMemberName::String(s) | TSEnumMemberName::ComputedString(s) => {
                    s.value.as_str().map(|v| member(v, m.span.start))
                }
                TSEnumMemberName::ComputedTemplateString(_) => None,
            })
            .collect()
    };
    let namespace_members = |n: &TSNamespaceDeclaration<'_>| -> Vec<ExportMember> {
        let TSNamespaceDeclarationBody::TSModuleBlock(block) = &n.body else {
            return vec![];
        };
        let mut list = vec![];
        for st in &block.body {
            if let Statement::ExportDeclaration(x) = st {
                list.extend(
                    declaration_names(&x.declaration)
                        .into_iter()
                        .map(|(name, start)| member(name, start)),
                );
            }
        }
        list
    };
    for st in &program.body {
        let decl = match st {
            Statement::ExportDeclaration(x) => Some(&x.declaration),
            _ => st.as_declaration(),
        };
        match decl {
            Some(Declaration::TSEnumDeclaration(e)) => {
                out.insert(e.id.name.to_string(), enum_members(e));
            }
            Some(Declaration::TSNamespaceDeclaration(n)) => {
                out.insert(n.id.name.to_string(), namespace_members(n));
            }
            _ => {}
        }
    }
    out
}

fn declaration_names<'a>(d: &'a Declaration<'_>) -> Vec<(&'a str, u32)> {
    match d {
        Declaration::VariableDeclaration(v) => v
            .declarations
            .iter()
            .flat_map(|d| d.id.get_binding_identifiers())
            .map(|i| (i.name.as_str(), i.span.start))
            .collect(),
        Declaration::FunctionDeclaration(f) => {
            f.id.iter()
                .map(|i| (i.name.as_str(), i.span.start))
                .collect()
        }
        Declaration::ClassDeclaration(c) => {
            c.id.iter()
                .map(|i| (i.name.as_str(), i.span.start))
                .collect()
        }
        Declaration::TSTypeAliasDeclaration(t) => vec![(t.id.name.as_str(), t.id.span.start)],
        Declaration::TSInterfaceDeclaration(t) => vec![(t.id.name.as_str(), t.id.span.start)],
        Declaration::TSEnumDeclaration(t) => vec![(t.id.name.as_str(), t.id.span.start)],
        Declaration::TSNamespaceDeclaration(t) => vec![(t.id.name.as_str(), t.id.span.start)],
        _ => vec![],
    }
}

struct AstVisitor<'b, 's> {
    b: &'b mut FactsBuilder<'s>,
    offset: u32,
    /// Start of the innermost enclosing statement, if any.
    stmt_start: Option<u32>,
    symbols: Option<Symbols>,
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

/// Whether the callee is the identifier or `a.b.c` member chain `name`, compared segment by
/// segment so no dotted string is built for the (many) calls that are not of interest.
fn callee_is(e: &Expression<'_>, name: &str) -> bool {
    match e.without_parentheses() {
        Expression::Identifier(i) => i.name == name,
        Expression::StaticMemberExpression(m) => name
            .strip_suffix(m.property.name.as_str())
            .and_then(|rest| rest.strip_suffix('.'))
            .is_some_and(|rest| callee_is(&m.object, rest)),
        _ => false,
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

/// `new URL("./x", import.meta.url)` -> `"./x"`. Directories (`"."`, `"../"`) are not modules.
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
    static_string(first).filter(|s| !matches!(*s, "." | ".." | "") && !s.ends_with('/'))
}

fn relative(spec: &str) -> String {
    if spec.starts_with('.') || spec.starts_with('/') || spec.contains(':') {
        spec.to_string()
    } else {
        format!("./{spec}")
    }
}

fn export_name_of(n: &ModuleExportName<'_>) -> String {
    match n {
        ModuleExportName::IdentifierName(i) => i.name.to_string(),
        ModuleExportName::IdentifierReference(i) => i.name.to_string(),
        ModuleExportName::StringLiteral(s) => s.value.as_str().unwrap_or_default().to_string(),
    }
}

impl Symbols {
    /// Record how to delete each specifier of `export { a, b }`: with the comma that separates
    /// it from a neighbour, or the whole statement when it is the only one.
    fn fix_specifiers(&mut self, specs: &[ExportSpecifier<'_>], statement: oxc_span::Span) {
        for (i, s) in specs.iter().enumerate() {
            let range = match (specs.len(), i) {
                (1, _) => (statement.start, statement.end),
                (n, i) if i + 1 < n => (s.span.start, specs[i + 1].span.start),
                (_, i) => (specs[i - 1].span.end, s.span.end),
            };
            self.list_fix(export_name_of(&s.exported), range);
        }
    }

    fn list_fix(&mut self, name: String, range: (u32, u32)) {
        self.listed_fixes
            .entry(name)
            .and_modify(|r| *r = None)
            .or_insert(Some(range));
    }

    /// Index of the import `name` is bound by, if it is an import binding.
    fn import_of(&self, name: &str) -> Option<usize> {
        self.import_idx.get(name).copied()
    }
}

impl<'a> Visit<'a> for AstVisitor<'_, '_> {
    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        if let Some(sym) = &mut self.symbols {
            if let Some(i) = sym.import_of(&it.name) {
                sym.opaque.insert(i);
            } else if sym.export_locals.contains(it.name.as_str()) {
                sym.referenced.insert(it.name.to_string());
            }
        }
    }

    /// `ns.a` reads one member of an import binding; it is not a use of the binding itself.
    fn visit_static_member_expression(&mut self, it: &StaticMemberExpression<'a>) {
        if let Some(sym) = &mut self.symbols
            && let Expression::Identifier(id) = &it.object
            && let Some(i) = sym.import_of(&id.name)
        {
            sym.members
                .entry(i)
                .or_default()
                .push(it.property.name.to_string());
            return;
        }
        walk::walk_static_member_expression(self, it);
    }

    /// `ns.T` in a type position.
    fn visit_ts_qualified_name(&mut self, it: &TSQualifiedName<'a>) {
        if let Some(sym) = &mut self.symbols
            && let TSTypeName::IdentifierReference(id) = &it.left
            && let Some(i) = sym.import_of(&id.name)
        {
            sym.members
                .entry(i)
                .or_default()
                .push(it.right.name.to_string());
            return;
        }
        walk::walk_ts_qualified_name(self, it);
    }

    /// `export { a, b }` lists names; it is not a use in the file. A re-exported namespace
    /// import is the exception: nothing narrows what the re-export's users read from it.
    fn visit_export_named_declaration(&mut self, it: &ExportNamedDeclaration<'a>) {
        let Some(sym) = &mut self.symbols else {
            return walk::walk_export_named_declaration(self, it);
        };
        sym.fix_specifiers(&it.specifiers, it.span);
        for spec in &it.specifiers {
            if let ModuleExportName::IdentifierReference(id) = &spec.local
                && let Some(i) = sym.import_of(&id.name)
                && self.b.imports[i].imported == "*"
            {
                sym.opaque.insert(i);
            }
        }
    }

    fn visit_export_from_declaration(&mut self, it: &ExportFromDeclaration<'a>) {
        if let Some(sym) = &mut self.symbols {
            sym.fix_specifiers(&it.specifiers, it.span);
        }
        walk::walk_export_from_declaration(self, it);
    }

    fn visit_export_all_declaration(&mut self, it: &ExportAllDeclaration<'a>) {
        if let (Some(sym), Some(name)) = (&mut self.symbols, &it.exported) {
            sym.list_fix(export_name_of(name), (it.span.start, it.span.end));
        }
        walk::walk_export_all_declaration(self, it);
    }

    fn visit_export_declaration(&mut self, it: &ExportDeclaration<'a>) {
        if let Some(sym) = &mut self.symbols {
            sym.keyword_fixes
                .insert(it.span.start, (it.span.start, it.declaration.span().start));
        }
        walk::walk_export_declaration(self, it);
    }

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
        } else {
            let first = it.arguments.first().and_then(|a| a.as_expression());
            let opts = self.b.opts;
            if let Some(spec) = first.and_then(static_string)
                && let Some(name) = opts.calls.iter().find(|c| callee_is(&it.callee, c))
            {
                let span = first.map_or(it.span, |e| e.span());
                let ms = if name.starts_with("require") {
                    ModuleSystem::Cjs
                } else {
                    ModuleSystem::Es6
                };
                self.push(spec, DepKinds::CALL, ms, span);
            }
            // AMD: define([...], fn) / define("name", [...], fn) / require([...], fn)
            if !self.b.has_module_syntax
                && (callee_is(&it.callee, "define") || callee_is(&it.callee, "require"))
            {
                let arr = it.arguments.iter().find_map(|a| {
                    match a.as_expression()?.without_parentheses() {
                        Expression::ArrayExpression(arr) => Some(arr),
                        _ => None,
                    }
                });
                if let Some(arr) = arr {
                    // dependency-cruiser calls both array forms `amd-define`; `amd-require`
                    // is reserved for CJS-style requires inside AMD modules.
                    let kind = DepKinds::AMD_DEFINE;
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
