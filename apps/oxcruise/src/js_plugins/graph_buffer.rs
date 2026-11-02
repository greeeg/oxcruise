//! Serialises the finished graph into one flat buffer for the JS plugin host.
//!
//! Plugins see the whole graph, so it crosses the napi boundary exactly once per run.
//! Everything is `u32` little-endian and 4-byte aligned so JS can wrap sections in
//! `Uint32Array` views without copying:
//!
//! ```text
//! header   26 x u32   magic, version, counts, section offsets (bytes)
//! strings  (n_strings + 1) x u32 offsets, then UTF-8 bytes padded to 4
//! modules  n_modules x 4 x u32   path, flags, package, distinct dependents
//! edges    n_edges   x 8 x u32   from, to, specifier, kinds lo, kinds hi, line, col, flags
//! out_off  (n_modules + 1) x u32   edges are sorted by `from`, so this indexes `edges`
//! in_off   (n_modules + 1) x u32
//! in_edges n_edges x u32           edge ids ordered by target module
//! scc      n_modules x u32         strongly connected component id of each module
//! packages n_packages x u32        string index of each workspace package name
//! exp_off  (n_modules + 1) x u32   exports are grouped by module, so this indexes `exports`
//! exports  n_exports x 6 x u32     name, flags, line, col, from module, imported name
//! name_off (n_edges + 1) x u32     used names of each edge, indexing `names`
//! names    n_names x u32           string index of each used name (`ns.a` is a member of `ns`)
//! mem_off  (n_exports + 1) x u32   used members of each export, indexing `members`
//! members  n_members x u32         string index of each member read (sorted by name)
//! ```
//!
//! Header: words 0-1 magic and version, 2-5 module, edge, string and package counts, 6-14 the byte
//! offsets of strings, string bytes, modules, edges, out_off, in_off, in_edges, scc and packages,
//! 15 the total size, 16-17 export and name counts, 18-21 the offsets of exp_off, exports,
//! name_off and names. Word 22 is 1 when symbols were extracted, 23-24 are the offsets of mem_off and
//! members, word 25 is reserved.
//!
//! Export flags: bits 0-1 kind (0 local, 1 re-export, 2 star, 3 namespace), bit 2 type, bit 3 used,
//! bit 4 every member is used (`E.A` for any `A`; the export's member list is then empty).
//! `from` is `NO_MODULE` and `imported` `NO_STRING` when they do not apply.
//!
//! Module flags: bits 0-3 kind (0 local, 1 package, 2 builtin, 3 unresolved), bit 4 not followed,
//! bit 5 parse error. Edge flags: bit 0 type-only, bit 1 on a cycle, bit 2 unresolved target,
//! bit 3 uses any name (`Dependency.names === "*"`; the edge's name list is then empty).

use oxcruise_core::{Edge, ExportKind, Graph, ModuleId, ModuleKind, UsedNames};
use oxcruise_graph::analyses::{ExportUsage, Use, distinct_dependents, strongly_connected};
use oxcruise_resolve::WorkspacePackage;
use oxcruise_rules::AnalysisCache;
use string_table::StringTable;

pub const MAGIC: u32 = 0x5247_584F; // "OXGR" little-endian
pub const VERSION: u32 = 3;
pub const NO_PACKAGE: u32 = u32::MAX;
pub const NO_MODULE: u32 = u32::MAX;
pub const NO_STRING: u32 = u32::MAX;
const HEADER_WORDS: usize = 26;

mod string_table {
    use std::collections::HashMap;

    /// Deduplicating string table.
    #[derive(Default)]
    pub struct StringTable {
        index: HashMap<String, u32>,
        pub strings: Vec<String>,
    }

    impl StringTable {
        pub fn intern(&mut self, s: &str) -> u32 {
            if let Some(&i) = self.index.get(s) {
                return i;
            }
            let i = self.strings.len() as u32;
            self.index.insert(s.to_string(), i);
            self.strings.push(s.to_string());
            i
        }
    }
}

pub fn encode(
    graph: &Graph,
    workspace: &[WorkspacePackage],
    usage: Option<&ExportUsage>,
) -> Vec<u8> {
    encode_with(graph, workspace, usage, &AnalysisCache::default())
}

/// [`encode`], reusing the cycles and dependent counts that `analyses` already holds.
pub fn encode_with(
    graph: &Graph,
    workspace: &[WorkspacePackage],
    usage: Option<&ExportUsage>,
    analyses: &AnalysisCache,
) -> Vec<u8> {
    let (n_mod, n_edge) = (graph.module_count(), graph.edge_count());
    let own_scc;
    let scc = match &analyses.scc_all {
        Some(scc) => scc,
        None => {
            own_scc = strongly_connected(graph, &|_: &Edge| true);
            &own_scc
        }
    };

    let mut strings = StringTable::default();
    let mut modules: Vec<u32> = Vec::with_capacity(n_mod * 4);
    for id in graph.module_ids() {
        let m = graph.module(id);
        let kind = match m.kind {
            ModuleKind::Local => 0,
            ModuleKind::Package => 1,
            ModuleKind::Builtin => 2,
            ModuleKind::Unresolved => 3,
        };
        let flags = kind | u32::from(m.not_followed) << 4 | u32::from(m.parse_error.is_some()) << 5;
        modules.extend([
            strings.intern(&m.path),
            flags,
            m.package.unwrap_or(NO_PACKAGE),
            match &analyses.dependents {
                Some(d) => d[id.idx()],
                None => distinct_dependents(graph, id),
            },
        ]);
    }

    let mut edges: Vec<u32> = Vec::with_capacity(n_edge * 8);
    for (i, e) in graph.edges().iter().enumerate() {
        let bits = e.kinds.bits();
        let cyclic = scc.is_cyclic_edge(graph, oxcruise_core::EdgeId(i as u32));
        let unresolved = graph.module(e.to).kind == ModuleKind::Unresolved;
        let flags = u32::from(e.is_type_only())
            | u32::from(cyclic) << 1
            | u32::from(unresolved) << 2
            | u32::from(e.used == UsedNames::All) << 3;
        edges.extend([
            e.from.0,
            e.to.0,
            strings.intern(&e.specifier),
            bits as u32,
            (bits >> 32) as u32,
            e.loc.map_or(0, |l| l.line),
            e.loc.map_or(0, |l| l.col),
            flags,
        ]);
    }

    // exports, grouped by module, and the used names of each edge
    let mut exp_off: Vec<u32> = Vec::with_capacity(n_mod + 1);
    let mut exports: Vec<u32> = vec![];
    let mut mem_off: Vec<u32> = vec![];
    let mut members: Vec<u32> = vec![];
    for id in graph.module_ids() {
        exp_off.push((exports.len() / 6) as u32);
        for x in &graph.module(id).exports {
            let kind = match x.kind {
                ExportKind::Local => 0,
                ExportKind::Reexport => 1,
                ExportKind::Star => 2,
                ExportKind::Namespace => 3,
            };
            let used = usage.is_some_and(|u| u.is_used(id, &x.name));
            let all_members = usage.is_some_and(|u| {
                u.all.contains(id.idx())
                    || matches!(u.used[id.idx()].get(&*x.name), Some(Use::Whole))
            });
            mem_off.push(members.len() as u32);
            if let Some(Use::Members(set)) = usage.and_then(|u| u.used[id.idx()].get(&*x.name))
                && !all_members
            {
                let mut sorted: Vec<&str> = set.iter().map(|m| &**m).collect();
                sorted.sort_unstable();
                members.extend(sorted.into_iter().map(|m| strings.intern(m)));
            }
            exports.extend([
                strings.intern(&x.name),
                kind | u32::from(x.is_type) << 2
                    | u32::from(used) << 3
                    | u32::from(all_members) << 4,
                x.loc.line,
                x.loc.col,
                x.from.map_or(NO_MODULE, |ModuleId(m)| m),
                x.imported
                    .as_deref()
                    .map_or(NO_STRING, |s| strings.intern(s)),
            ]);
        }
    }
    exp_off.push((exports.len() / 6) as u32);
    mem_off.push(members.len() as u32);
    let mut name_off: Vec<u32> = Vec::with_capacity(n_edge + 1);
    let mut names: Vec<u32> = vec![];
    for e in graph.edges() {
        name_off.push(names.len() as u32);
        if let UsedNames::Names(list) = &e.used {
            names.extend(list.iter().map(|n| strings.intern(n)));
        }
    }
    name_off.push(names.len() as u32);

    let packages: Vec<u32> = workspace.iter().map(|p| strings.intern(&p.name)).collect();
    let scc_ids: Vec<u32> = graph.module_ids().map(|m| scc.component(m)).collect();

    // string section
    let mut str_offsets: Vec<u32> = Vec::with_capacity(strings.strings.len() + 1);
    let mut str_bytes: Vec<u8> = vec![];
    for s in &strings.strings {
        str_offsets.push(str_bytes.len() as u32);
        str_bytes.extend_from_slice(s.as_bytes());
    }
    str_offsets.push(str_bytes.len() as u32);
    while !str_bytes.len().is_multiple_of(4) {
        str_bytes.push(0);
    }

    let in_edges: Vec<u32> = graph.in_edge_table().iter().map(|e| e.0).collect();

    // layout
    let mut offset = HEADER_WORDS * 4;
    let mut place = |bytes: usize| {
        let at = offset;
        offset += bytes;
        at as u32
    };
    let off_str_offsets = place(str_offsets.len() * 4);
    let off_str_bytes = place(str_bytes.len());
    let off_modules = place(modules.len() * 4);
    let off_edges = place(edges.len() * 4);
    let off_out = place(graph.out_offsets().len() * 4);
    let off_in = place(graph.in_offsets().len() * 4);
    let off_in_edges = place(in_edges.len() * 4);
    let off_scc = place(scc_ids.len() * 4);
    let off_packages = place(packages.len() * 4);
    let off_exp_off = place(exp_off.len() * 4);
    let off_exports = place(exports.len() * 4);
    let off_name_off = place(name_off.len() * 4);
    let off_names = place(names.len() * 4);
    let off_mem_off = place(mem_off.len() * 4);
    let off_members = place(members.len() * 4);
    let total = offset;

    let header: [u32; HEADER_WORDS] = [
        MAGIC,
        VERSION,
        n_mod as u32,
        n_edge as u32,
        strings.strings.len() as u32,
        packages.len() as u32,
        off_str_offsets,
        off_str_bytes,
        off_modules,
        off_edges,
        off_out,
        off_in,
        off_in_edges,
        off_scc,
        off_packages,
        total as u32,
        (exports.len() / 6) as u32,
        names.len() as u32,
        off_exp_off,
        off_exports,
        off_name_off,
        off_names,
        u32::from(usage.is_some()),
        off_mem_off,
        off_members,
        0,
    ];

    let mut out = Vec::with_capacity(total);
    let words = |out: &mut Vec<u8>, w: &[u32]| {
        w.iter()
            .for_each(|x| out.extend_from_slice(&x.to_le_bytes()))
    };
    words(&mut out, &header);
    words(&mut out, &str_offsets);
    out.extend_from_slice(&str_bytes);
    words(&mut out, &modules);
    words(&mut out, &edges);
    words(&mut out, graph.out_offsets());
    words(&mut out, graph.in_offsets());
    words(&mut out, &in_edges);
    words(&mut out, &scc_ids);
    words(&mut out, &packages);
    words(&mut out, &exp_off);
    words(&mut out, &exports);
    words(&mut out, &name_off);
    words(&mut out, &names);
    words(&mut out, &mem_off);
    words(&mut out, &members);
    debug_assert_eq!(out.len(), total);
    out
}

#[cfg(test)]
pub(crate) mod decode {
    //! A deliberately independent decoder, so the round-trip test checks the layout
    //! documented above rather than the encoder against itself.

    pub struct Decoded<'a>(pub &'a [u8]);

    impl<'a> Decoded<'a> {
        pub fn word(&self, byte_offset: usize) -> u32 {
            u32::from_le_bytes(self.0[byte_offset..byte_offset + 4].try_into().unwrap())
        }
        pub fn header(&self, i: usize) -> u32 {
            self.word(i * 4)
        }
        pub fn string(&self, i: usize) -> &'a str {
            let offs = self.header(6) as usize;
            let bytes = self.header(7) as usize;
            let (a, b) = (
                self.word(offs + i * 4) as usize,
                self.word(offs + (i + 1) * 4) as usize,
            );
            std::str::from_utf8(&self.0[bytes + a..bytes + b]).unwrap()
        }
        pub fn module(&self, i: usize) -> (&'a str, u32, u32, u32) {
            let base = self.header(8) as usize + i * 16;
            (
                self.string(self.word(base) as usize),
                self.word(base + 4),
                self.word(base + 8),
                self.word(base + 12),
            )
        }
        /// `(from, to, specifier, kinds, line, col, flags)`
        pub fn edge(&self, i: usize) -> (u32, u32, &'a str, u64, u32, u32, u32) {
            let base = self.header(9) as usize + i * 32;
            let kinds = u64::from(self.word(base + 12)) | u64::from(self.word(base + 16)) << 32;
            (
                self.word(base),
                self.word(base + 4),
                self.string(self.word(base + 8) as usize),
                kinds,
                self.word(base + 20),
                self.word(base + 24),
                self.word(base + 28),
            )
        }
        pub fn out_off(&self, i: usize) -> u32 {
            self.word(self.header(10) as usize + i * 4)
        }
        pub fn in_off(&self, i: usize) -> u32 {
            self.word(self.header(11) as usize + i * 4)
        }
        pub fn in_edge(&self, i: usize) -> u32 {
            self.word(self.header(12) as usize + i * 4)
        }
        pub fn scc(&self, i: usize) -> u32 {
            self.word(self.header(13) as usize + i * 4)
        }
        pub fn package(&self, i: usize) -> &'a str {
            self.string(self.word(self.header(14) as usize + i * 4) as usize)
        }
        /// `(name, flags, line, col, from, imported)` of the exports of module `m`.
        #[allow(clippy::type_complexity)]
        pub fn exports(&self, m: usize) -> Vec<(&'a str, u32, u32, u32, u32, Option<&'a str>)> {
            let off = |i: usize| self.word(self.header(18) as usize + i * 4) as usize;
            (off(m)..off(m + 1))
                .map(|i| {
                    let base = self.header(19) as usize + i * 24;
                    let imported = self.word(base + 20);
                    (
                        self.string(self.word(base) as usize),
                        self.word(base + 4),
                        self.word(base + 8),
                        self.word(base + 12),
                        self.word(base + 16),
                        (imported != u32::MAX).then(|| self.string(imported as usize)),
                    )
                })
                .collect()
        }
        /// The used members of the `i`-th export in the whole buffer (exports are grouped by
        /// module in order).
        pub fn members(&self, i: usize) -> Vec<&'a str> {
            let off = |i: usize| self.word(self.header(23) as usize + i * 4) as usize;
            (off(i)..off(i + 1))
                .map(|j| self.string(self.word(self.header(24) as usize + j * 4) as usize))
                .collect()
        }
        /// The used names of edge `e`.
        pub fn names(&self, e: usize) -> Vec<&'a str> {
            let off = |i: usize| self.word(self.header(20) as usize + i * 4) as usize;
            (off(e)..off(e + 1))
                .map(|i| self.string(self.word(self.header(21) as usize + i * 4) as usize))
                .collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::decode::Decoded;
    use super::*;
    use oxcruise_core::{DepKinds, Loc, Module, ModuleId, ModuleSystem};

    fn sample() -> Graph {
        let mut a = Module::new("src/ä.ts", ModuleKind::Local); // non-ASCII path
        a.package = Some(1);
        let mut b = Module::new("src/b.ts", ModuleKind::Local);
        let export = |name: &str, kind, from, imported: Option<&str>| oxcruise_core::Export {
            name: name.into(),
            kind,
            is_type: name == "T",
            from,
            imported: imported.map(Into::into),
            loc: Loc { line: 7, col: 14 },
            suppressed: vec![],
            members: vec![],
            used_in_file: false,
            duplicate_of: None,
            fix: None,
        };
        b.exports = vec![
            export("a", ExportKind::Local, None, None),
            export("T", ExportKind::Local, None, None),
            export("z", ExportKind::Reexport, Some(ModuleId(1)), Some("y")),
        ];
        let modules = vec![
            a,
            b,
            Module::new("fs", ModuleKind::Builtin),
            Module::new("./missing", ModuleKind::Unresolved),
        ];
        let e = |from, to, spec: &str, kinds: DepKinds, line| Edge {
            from: ModuleId(from),
            to: ModuleId(to),
            specifier: spec.into(),
            kinds,
            module_system: ModuleSystem::Es6,
            loc: Some(Loc { line, col: 3 }),
            suppressed: vec![],
            used: oxcruise_core::UsedNames::All,
        };
        // module order after sort: "./missing", "fs", "src/b.ts", "src/ä.ts"
        let edges = vec![
            {
                let mut edge = e(0, 1, "./b", DepKinds::LOCAL | DepKinds::IMPORT, 1);
                edge.used = UsedNames::Names(["a".into(), "ns.é".into()].into());
                edge
            },
            e(
                1,
                0,
                "./ä",
                DepKinds::LOCAL | DepKinds::TYPE_ONLY | DepKinds::IMPORT,
                2,
            ),
            e(0, 2, "node:fs", DepKinds::CORE | DepKinds::REQUIRE, 3),
            e(0, 3, "./missing", DepKinds::UNKNOWN | DepKinds::URL, 4),
        ];
        Graph::build(modules, edges)
    }

    fn workspace() -> Vec<WorkspacePackage> {
        ["pkg-zero", "pkg-ünï"]
            .iter()
            .map(|n| WorkspacePackage {
                name: (*n).into(),
                dir: "/x".into(),
                rel_dir: "x".into(),
            })
            .collect()
    }

    #[test]
    fn round_trips_through_an_independent_decoder() {
        let g = sample();
        let usage = oxcruise_graph::analyses::export_usage(
            &g,
            &oxcruise_graph::analyses::BitSet::new(g.module_count()),
            &|_| false,
        );
        let buf = encode(&g, &workspace(), Some(&usage));
        assert_eq!(buf.len() % 4, 0);
        let d = Decoded(&buf);
        assert_eq!((d.header(0), d.header(1)), (MAGIC, VERSION));
        assert_eq!(d.header(15) as usize, buf.len());
        assert_eq!(
            (d.header(2) as usize, d.header(3) as usize),
            (g.module_count(), g.edge_count())
        );

        for (i, m) in g.modules().iter().enumerate() {
            let (path, flags, pkg, dependents) = d.module(i);
            assert_eq!(path, m.path);
            let kind = match m.kind {
                ModuleKind::Local => 0,
                ModuleKind::Package => 1,
                ModuleKind::Builtin => 2,
                ModuleKind::Unresolved => 3,
            };
            assert_eq!(flags & 0xF, kind);
            assert_eq!(pkg, m.package.unwrap_or(NO_PACKAGE));
            assert_eq!(dependents, distinct_dependents(&g, ModuleId(i as u32)));
        }
        for (i, e) in g.edges().iter().enumerate() {
            let (from, to, spec, kinds, line, col, flags) = d.edge(i);
            assert_eq!((from, to), (e.from.0, e.to.0));
            assert_eq!(spec, e.specifier);
            assert_eq!(kinds, e.kinds.bits(), "u64 kinds survive the lo/hi split");
            assert_eq!((line, col), (e.loc.unwrap().line, e.loc.unwrap().col));
            assert_eq!(flags & 1 == 1, e.is_type_only());
        }
        // CSR sections agree with the graph
        for m in g.module_ids() {
            assert_eq!(
                d.out_off(m.idx()) as usize,
                g.out_offsets()[m.idx()] as usize
            );
            assert_eq!(d.in_off(m.idx()) as usize, g.in_offsets()[m.idx()] as usize);
        }
        assert_eq!(d.out_off(g.module_count()) as usize, g.edge_count());
        for (i, id) in g.in_edge_table().iter().enumerate() {
            assert_eq!(d.in_edge(i), id.0);
        }
        // the ./b <-> ./ä cycle shares a component and is flagged
        let (b, a) = (g.find("src/b.ts").unwrap(), g.find("src/ä.ts").unwrap());
        assert_eq!(d.scc(a.idx()), d.scc(b.idx()));
        let cyclic: Vec<_> = (0..g.edge_count())
            .filter(|&i| d.edge(i).6 & 2 != 0)
            .collect();
        assert_eq!(cyclic.len(), 2);
        // exports with their flags, and the names each edge uses
        let listed = d.exports(b.idx());
        assert_eq!(listed.len(), 3);
        // sorted by name: `T`, `a`, `z`
        assert_eq!(
            (listed[0].0, listed[0].1 & 3, listed[0].1 >> 2 & 1),
            ("T", 0, 1)
        );
        assert_eq!((listed[1].0, listed[1].1 >> 2 & 1), ("a", 0));
        assert_eq!(listed[1].1 >> 3 & 1, 1, "`a` is used by ./b's importer");
        assert_eq!(listed[0].1 >> 3 & 1, 0, "nothing uses `T`");
        assert_eq!((listed[0].2, listed[0].3), (7, 14));
        assert_eq!(
            (listed[2].1 & 3, listed[2].4, listed[2].5),
            (1, b.0, Some("y"))
        );
        assert_eq!(listed[0].4, NO_MODULE);
        assert!(d.exports(0).is_empty());
        for (i, e) in g.edges().iter().enumerate() {
            let expected: Vec<String> = match &e.used {
                UsedNames::All => vec![],
                UsedNames::Names(n) => n.iter().map(|s| s.to_string()).collect(),
            };
            assert_eq!(d.names(i), expected);
            assert_eq!(d.edge(i).6 >> 3 & 1 == 1, e.used == UsedNames::All);
        }
        // unresolved flag, packages
        assert!((0..g.edge_count()).any(|i| d.edge(i).6 & 4 != 0));
        assert_eq!((d.package(0), d.package(1)), ("pkg-zero", "pkg-ünï"));
    }

    #[test]
    fn used_members_are_listed_unless_every_member_is_used() {
        let export = |name: &str| oxcruise_core::Export {
            name: name.into(),
            kind: ExportKind::Local,
            is_type: false,
            from: None,
            imported: None,
            loc: Loc { line: 1, col: 1 },
            suppressed: vec![],
            members: vec![],
            used_in_file: false,
            duplicate_of: None,
            fix: None,
        };
        let mut lib = Module::new("lib.ts", ModuleKind::Local);
        lib.exports = vec![export("E"), export("F"), export("G")];
        let importer = |names: UsedNames| Edge {
            from: ModuleId(0),
            to: ModuleId(1),
            specifier: "./lib".into(),
            kinds: DepKinds::LOCAL | DepKinds::IMPORT,
            module_system: ModuleSystem::Es6,
            loc: Some(Loc { line: 1, col: 1 }),
            suppressed: vec![],
            used: names,
        };
        let g = Graph::build(
            vec![Module::new("main.ts", ModuleKind::Local), lib],
            vec![
                importer(UsedNames::Names(
                    ["E.B".into(), "E.A".into(), "F".into()].into(),
                )),
                importer(UsedNames::Names(["G.x".into(), "G".into()].into())),
            ],
        );
        let usage = oxcruise_graph::analyses::export_usage(
            &g,
            &oxcruise_graph::analyses::BitSet::new(g.module_count()),
            &|_| false,
        );
        let buf = encode(&g, &[], Some(&usage));
        let d = Decoded(&buf);
        let lib = g.find("lib.ts").unwrap();
        let listed = d.exports(lib.idx());
        // the module's first export in the buffer-wide numbering of `members`
        let first = d.word(d.header(18) as usize + lib.idx() * 4) as usize;
        let at = |i: usize| (listed[i].1 >> 3 & 3, d.members(first + i));
        assert_eq!(at(0), (1, vec!["A", "B"]), "E: only A and B are read");
        assert_eq!(at(1), (3, vec![]), "F: used whole");
        assert_eq!(at(2), (3, vec![]), "G: a whole use covers its members");
    }

    #[test]
    fn empty_graph_encodes() {
        let buf = encode(&Graph::build(vec![], vec![]), &[], None);
        let d = Decoded(&buf);
        assert_eq!((d.header(2), d.header(3)), (0, 0));
        assert_eq!(d.header(15) as usize, buf.len());
    }
}

// Unix only: the monorepo fixture links `node_modules/@acme/lib` with a symlink, which Windows
// checkouts turn into a plain file, so the graph (and the bytes) differ there.
#[cfg(all(test, unix))]
mod fixtures {
    //! The JS facade tests decode `monorepo.graph.bin` and compare with `monorepo.graph.json`.
    //! Both come from this encoder; this test fails when they are stale.
    //! Regenerate: `UPDATE_FIXTURES=1 cargo test -p oxcruise graph_buffer::fixtures`

    use super::*;
    use oxcruise_graph::{CruiseOptions, cruise};

    fn fixture_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src-js/__fixtures__")
    }

    /// `"*"` when any member may be read, else the members that are, sorted.
    fn used_members(usage: &ExportUsage, m: ModuleId, name: &str) -> serde_json::Value {
        match usage.used[m.idx()].get(name) {
            _ if usage.all.contains(m.idx()) => serde_json::json!("*"),
            Some(Use::Whole) => serde_json::json!("*"),
            Some(Use::Members(set)) => {
                let mut sorted: Vec<&str> = set.iter().map(|s| &**s).collect();
                sorted.sort_unstable();
                serde_json::json!(sorted)
            }
            None => serde_json::json!([]),
        }
    }

    /// The facts the JS test checks, in plain JSON.
    fn describe(
        g: &Graph,
        workspace: &[WorkspacePackage],
        usage: &ExportUsage,
    ) -> serde_json::Value {
        let scc = strongly_connected(g, &|_: &Edge| true);
        serde_json::json!({
            "packages": workspace.iter().map(|p| &p.name).collect::<Vec<_>>(),
            "modules": g.modules().iter().enumerate().map(|(i, m)| serde_json::json!({
                "path": m.path,
                "kind": format!("{:?}", m.kind).to_lowercase(),
                "package": m.package.and_then(|p| workspace.get(p as usize)).map(|p| p.name.clone()),
                "notFollowed": m.not_followed,
                "dependents": distinct_dependents(g, oxcruise_core::ModuleId(i as u32)),
                "exports": m.exports.iter().map(|x| serde_json::json!({
                    "name": x.name,
                    "kind": match x.kind {
                        ExportKind::Local => "local",
                        ExportKind::Reexport => "reexport",
                        ExportKind::Star => "star",
                        ExportKind::Namespace => "namespace",
                    },
                    "type": x.is_type,
                    "line": x.loc.line,
                    "col": x.loc.col,
                    "from": x.from.map(|f| g.module(f).path.clone()),
                    "imported": x.imported,
                    "used": usage.is_used(oxcruise_core::ModuleId(i as u32), &x.name),
                    "usedMembers": used_members(usage, oxcruise_core::ModuleId(i as u32), &x.name),
                })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "edges": g.edges().iter().enumerate().map(|(i, e)| serde_json::json!({
                "from": g.module(e.from).path,
                "to": g.module(e.to).path,
                "specifier": e.specifier,
                "kinds": e.kinds.names().collect::<Vec<_>>(),
                "line": e.loc.map(|l| l.line),
                "col": e.loc.map(|l| l.col),
                "typeOnly": e.is_type_only(),
                "circular": scc.is_cyclic_edge(g, oxcruise_core::EdgeId(i as u32)),
                "names": match &e.used {
                    UsedNames::All => serde_json::json!("*"),
                    UsedNames::Names(n) => serde_json::json!(n),
                },
            })).collect::<Vec<_>>(),
        })
    }

    #[test]
    fn checked_in_fixture_matches_the_encoder() {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/monorepo");
        let mut opts = CruiseOptions::new(root.canonicalize().unwrap(), vec!["packages".into()]);
        opts.extract.symbols = true;
        opts.manifests = true;
        let out = cruise(&opts).unwrap();
        let entries = oxcruise_rules::entries::compute(
            &out.graph,
            &oxcruise_rules::entries::EntryInputs {
                manifests: &out.manifests,
                conventions: &[oxcruise_rules::ConventionFilter::default()],
                plugin_entries: &[],
            },
        );
        let usage = oxcruise_graph::analyses::export_usage(&out.graph, &entries.all(), &|_| false);
        let bin = encode(&out.graph, &out.workspace, Some(&usage));
        let json = serde_json::to_string_pretty(&describe(&out.graph, &out.workspace, &usage))
            .unwrap()
            + "\n";
        let dir = fixture_dir();
        if std::env::var_os("UPDATE_FIXTURES").is_some() {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("monorepo.graph.bin"), &bin).unwrap();
            std::fs::write(dir.join("monorepo.graph.json"), &json).unwrap();
        }
        assert_eq!(
            std::fs::read(dir.join("monorepo.graph.bin")).unwrap_or_default(),
            bin,
            "stale monorepo.graph.bin"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("monorepo.graph.json")).unwrap_or_default(),
            json,
            "stale monorepo.graph.json"
        );
    }
}
