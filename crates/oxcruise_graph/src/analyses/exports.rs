//! Which exports of which modules are used: one forward worklist over `(module, name)` marks.
//!
//! Knip walks up the importers once per export. Here every importing edge marks the names it
//! uses once, and a mark travels through re-exports to the module that really defines the
//! name. Each mark is set once, so the work is linear in marks and re-export cycles end.

use oxcruise_core::{ExportKind, Graph, ModuleId, ModuleKind, UsedNames};
use rustc_hash::{FxHashMap, FxHashSet};

use super::BitSet;

/// How the importers use one export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Use {
    /// The export itself: any member may be read.
    Whole,
    /// Only these members are read (`E.A`, `ns.a`).
    Members(FxHashSet<Box<str>>),
}

#[derive(Debug, Clone)]
pub struct ExportUsage {
    /// Modules every export of which is used (entry points and `import *` users).
    pub all: BitSet,
    /// Per module, the exports that are used and how.
    pub used: Vec<FxHashMap<Box<str>, Use>>,
}

impl ExportUsage {
    /// Nothing is used (for graphs without symbols that still need an answer).
    pub fn empty(modules: usize) -> Self {
        ExportUsage {
            all: BitSet::new(modules),
            used: vec![FxHashMap::default(); modules],
        }
    }

    pub fn is_used(&self, m: ModuleId, name: &str) -> bool {
        self.all.contains(m.idx()) || self.used[m.idx()].contains_key(name)
    }

    /// Whether member `member` of export `name` is read. Members of an export that is used as a
    /// whole count as read.
    pub fn is_member_used(&self, m: ModuleId, name: &str, member: &str) -> bool {
        self.all.contains(m.idx())
            || match self.used[m.idx()].get(name) {
                Some(Use::Whole) => true,
                Some(Use::Members(set)) => set.contains(member),
                None => false,
            }
    }
}

enum Mark {
    All(ModuleId),
    Name(ModuleId, Box<str>),
}

struct Walk<'g> {
    g: &'g Graph,
    out: ExportUsage,
    seen: FxHashSet<(u32, Box<str>)>,
    queue: Vec<Mark>,
}

impl Walk<'_> {
    fn all(&mut self, m: ModuleId) {
        if self.g.module(m).kind == ModuleKind::Source && self.out.all.insert(m.idx()) {
            self.queue.push(Mark::All(m));
        }
    }

    /// Mark `name` (`export` or `export.member`) of `m` as used.
    fn name(&mut self, m: ModuleId, name: &str) {
        if self.g.module(m).kind != ModuleKind::Source || self.out.all.contains(m.idx()) {
            return;
        }
        if !self.seen.insert((m.0, name.into())) {
            return;
        }
        let (head, rest) = name.split_once('.').unwrap_or((name, ""));
        let slot = &mut self.out.used[m.idx()];
        if rest.is_empty() {
            slot.insert(head.into(), Use::Whole);
        } else {
            let member = rest.split('.').next().unwrap_or(rest);
            match slot
                .entry(head.into())
                .or_insert_with(|| Use::Members(FxHashSet::default()))
            {
                Use::Members(set) => {
                    set.insert(member.into());
                }
                Use::Whole => {}
            }
        }
        self.queue.push(Mark::Name(m, name.into()));
    }

    fn step(&mut self, mark: Mark) {
        let g = self.g;
        match mark {
            Mark::All(m) => {
                for x in &g.module(m).exports {
                    let Some(t) = x.from else { continue };
                    match x.kind {
                        ExportKind::Reexport => {
                            let imported = x.imported.as_deref().unwrap_or(&x.name);
                            self.name(t, imported);
                        }
                        ExportKind::Namespace | ExportKind::Star => self.all(t),
                        ExportKind::Local => {}
                    }
                }
            }
            Mark::Name(m, name) => {
                let (head, rest) = name.split_once('.').unwrap_or((&name, ""));
                let module = g.module(m);
                let declared = module.exports_named(head);
                if declared.is_empty() {
                    // not declared here: `export *` may supply it, except `default`
                    if head != "default" {
                        for star in module.exports_named("*") {
                            if let Some(t) = star.from {
                                self.name(t, &name);
                            }
                        }
                    }
                    return;
                }
                for x in declared {
                    let Some(t) = x.from else { continue };
                    match x.kind {
                        ExportKind::Reexport => {
                            let imported = x.imported.as_deref().unwrap_or(head);
                            if rest.is_empty() {
                                self.name(t, imported);
                            } else {
                                self.name(t, &format!("{imported}.{rest}"));
                            }
                        }
                        ExportKind::Namespace if rest.is_empty() => self.all(t),
                        ExportKind::Namespace => self.name(t, rest),
                        ExportKind::Local | ExportKind::Star => {}
                    }
                }
            }
        }
    }
}

/// Which exports are used, given the modules whose exports are public (`entries`). Importers
/// for which `ignore_importer` holds contribute nothing, which is how production mode leaves
/// tests out.
pub fn export_usage(
    g: &Graph,
    entries: &BitSet,
    ignore_importer: &dyn Fn(ModuleId) -> bool,
) -> ExportUsage {
    let n = g.module_count();
    let mut w = Walk {
        g,
        out: ExportUsage {
            all: BitSet::new(n),
            used: vec![FxHashMap::default(); n],
        },
        seen: FxHashSet::default(),
        queue: vec![],
    };
    for m in entries.iter() {
        w.all(ModuleId(m as u32));
    }
    for e in g.edges() {
        if ignore_importer(e.from) {
            continue;
        }
        match &e.used {
            UsedNames::All => w.all(e.to),
            UsedNames::Names(names) => {
                for name in names.iter() {
                    w.name(e.to, name);
                }
            }
        }
    }
    while let Some(mark) = w.queue.pop() {
        w.step(mark);
    }
    w.out
}

#[cfg(test)]
mod tests {
    use oxcruise_core::{DepKinds, Edge, Export, Loc, Module, ModuleSystem};

    use super::*;

    /// `a.ts: a, export { x as y } from b.ts, export * from c.ts, export * as ns from d.ts`
    struct Fixture {
        modules: Vec<Module>,
        edges: Vec<Edge>,
    }

    fn export(name: &str, kind: ExportKind, from: Option<u32>, imported: Option<&str>) -> Export {
        Export {
            name: name.into(),
            kind,
            is_type: false,
            from: from.map(ModuleId),
            imported: imported.map(Into::into),
            loc: Loc::default(),
            suppressed: vec![],
            members: vec![],
            used_in_file: false,
            duplicate_of: None,
            fix: None,
        }
    }

    impl Fixture {
        fn new(paths: &[&str]) -> Self {
            Fixture {
                modules: paths
                    .iter()
                    .map(|p| Module::new(*p, ModuleKind::Source))
                    .collect(),
                edges: vec![],
            }
        }
        fn exports(mut self, module: u32, list: Vec<Export>) -> Self {
            self.modules[module as usize].exports = list;
            self
        }
        /// `from` imports `names` of `to` (`*` = everything).
        fn import(mut self, from: u32, to: u32, names: &[&str]) -> Self {
            self.edges.push(Edge {
                from: ModuleId(from),
                to: ModuleId(to),
                specifier: format!("./{to}"),
                kinds: DepKinds::IMPORT | DepKinds::LOCAL,
                module_system: ModuleSystem::Es6,
                loc: None,
                suppressed: vec![],
                used: if names == ["*"] {
                    UsedNames::All
                } else {
                    UsedNames::Names(names.iter().map(|n| (*n).into()).collect())
                },
            });
            self
        }
        /// Paths are given in sorted order, so module ids equal indices.
        fn usage(self, entries: &[u32]) -> (Graph, ExportUsage) {
            let g = Graph::build(self.modules, self.edges);
            let mut set = BitSet::new(g.module_count());
            for &e in entries {
                set.insert(e as usize);
            }
            let u = export_usage(&g, &set, &|_| false);
            (g, u)
        }
    }

    fn used(u: &ExportUsage, m: u32, name: &str) -> bool {
        u.is_used(ModuleId(m), name)
    }

    #[test]
    fn a_direct_import_uses_only_what_it_names() {
        let (_, u) = Fixture::new(&["0.ts", "1.ts"])
            .exports(
                1,
                vec![
                    export("a", ExportKind::Local, None, None),
                    export("b", ExportKind::Local, None, None),
                ],
            )
            .import(0, 1, &["a"])
            .usage(&[]);
        assert!(used(&u, 1, "a"));
        assert!(!used(&u, 1, "b"));
    }

    #[test]
    fn a_barrel_passes_use_to_the_defining_module() {
        // 0 imports `y` from barrel 1, which has `export { x as y } from 2`
        let (_, u) = Fixture::new(&["0.ts", "1.ts", "2.ts"])
            .exports(
                1,
                vec![export("y", ExportKind::Reexport, Some(2), Some("x"))],
            )
            .exports(
                2,
                vec![
                    export("x", ExportKind::Local, None, None),
                    export("z", ExportKind::Local, None, None),
                ],
            )
            .import(0, 1, &["y"])
            .import(1, 2, &[])
            .usage(&[]);
        assert!(used(&u, 2, "x"));
        assert!(!used(&u, 2, "z"));
        assert!(used(&u, 1, "y"));
    }

    #[test]
    fn star_exports_forward_names_but_not_default() {
        let (_, u) = Fixture::new(&["0.ts", "1.ts", "2.ts"])
            .exports(1, vec![export("*", ExportKind::Star, Some(2), None)])
            .exports(
                2,
                vec![
                    export("a", ExportKind::Local, None, None),
                    export("default", ExportKind::Local, None, None),
                ],
            )
            .import(0, 1, &["a", "default"])
            .usage(&[]);
        assert!(used(&u, 2, "a"));
        assert!(!used(&u, 2, "default"));
    }

    #[test]
    fn a_local_export_shadows_a_star() {
        let (_, u) = Fixture::new(&["0.ts", "1.ts", "2.ts"])
            .exports(
                1,
                vec![
                    export("*", ExportKind::Star, Some(2), None),
                    export("a", ExportKind::Local, None, None),
                ],
            )
            .exports(2, vec![export("a", ExportKind::Local, None, None)])
            .import(0, 1, &["a"])
            .usage(&[]);
        assert!(!used(&u, 2, "a"));
    }

    #[test]
    fn a_namespace_reexport_forwards_member_reads() {
        // 0: `import { ns } from 1; ns.a`   1: `export * as ns from 2`
        let (_, u) = Fixture::new(&["0.ts", "1.ts", "2.ts"])
            .exports(1, vec![export("ns", ExportKind::Namespace, Some(2), None)])
            .exports(
                2,
                vec![
                    export("a", ExportKind::Local, None, None),
                    export("b", ExportKind::Local, None, None),
                ],
            )
            .import(0, 1, &["ns.a"])
            .usage(&[]);
        assert!(used(&u, 2, "a"));
        assert!(!used(&u, 2, "b"));
    }

    #[test]
    fn a_namespace_reexport_used_whole_uses_everything() {
        let (_, u) = Fixture::new(&["0.ts", "1.ts", "2.ts"])
            .exports(1, vec![export("ns", ExportKind::Namespace, Some(2), None)])
            .exports(2, vec![export("b", ExportKind::Local, None, None)])
            .import(0, 1, &["ns"])
            .usage(&[]);
        assert!(used(&u, 2, "b"));
    }

    #[test]
    fn enum_members_are_tracked_apart_from_the_enum() {
        let (_, u) = Fixture::new(&["0.ts", "1.ts"])
            .exports(1, vec![export("E", ExportKind::Local, None, None)])
            .import(0, 1, &["E.A"])
            .usage(&[]);
        let e = ModuleId(1);
        assert!(u.is_used(e, "E"));
        assert!(u.is_member_used(e, "E", "A"));
        assert!(!u.is_member_used(e, "E", "B"));

        let (_, u) = Fixture::new(&["0.ts", "1.ts"])
            .exports(1, vec![export("E", ExportKind::Local, None, None)])
            .import(0, 1, &["E.A", "E"])
            .usage(&[]);
        assert!(u.is_member_used(e, "E", "B"));
    }

    #[test]
    fn entry_modules_are_used_and_pass_it_on() {
        let (_, u) = Fixture::new(&["0.ts", "1.ts"])
            .exports(
                0,
                vec![export("a", ExportKind::Reexport, Some(1), Some("a"))],
            )
            .exports(
                1,
                vec![
                    export("a", ExportKind::Local, None, None),
                    export("b", ExportKind::Local, None, None),
                ],
            )
            .import(0, 1, &[])
            .usage(&[0]);
        assert!(used(&u, 0, "anything"));
        assert!(used(&u, 1, "a"));
        assert!(!used(&u, 1, "b"));
    }

    #[test]
    fn reexport_cycles_end() {
        let (_, u) = Fixture::new(&["0.ts", "1.ts", "2.ts"])
            .exports(1, vec![export("*", ExportKind::Star, Some(2), None)])
            .exports(
                2,
                vec![
                    export("*", ExportKind::Star, Some(1), None),
                    export("a", ExportKind::Local, None, None),
                ],
            )
            .import(0, 1, &["a", "missing"])
            .usage(&[]);
        assert!(used(&u, 2, "a"));
    }

    #[test]
    fn ignored_importers_use_nothing() {
        let f = Fixture::new(&["0.ts", "1.ts"])
            .exports(1, vec![export("a", ExportKind::Local, None, None)])
            .import(0, 1, &["a"]);
        let g = Graph::build(f.modules, f.edges);
        let u = export_usage(&g, &BitSet::new(2), &|m| m == ModuleId(0));
        assert!(!used(&u, 1, "a"));
    }
}
