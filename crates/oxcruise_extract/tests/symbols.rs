//! What `ExtractOptions::symbols` collects: exports with locations, and how imports are used.

use std::path::Path;

use oxc_allocator::Allocator;
use oxcruise_extract::{ExportFact, ExportKind, ExtractOptions, FileFacts, UsedRefs, extract};

fn facts(file: &str, source: &str) -> FileFacts {
    let opts = ExtractOptions {
        symbols: true,
        ..Default::default()
    };
    extract(Path::new(file), source, &opts, &Allocator::default())
}

fn export<'a>(f: &'a FileFacts, name: &str) -> &'a ExportFact {
    f.exports
        .iter()
        .find(|e| e.name == name)
        .unwrap_or_else(|| panic!("no export `{name}` in {:?}", f.exports))
}

fn refs(f: &FileFacts, local: &str) -> UsedRefs {
    f.imports
        .iter()
        .find(|i| i.local == local)
        .unwrap_or_else(|| panic!("no import `{local}`"))
        .refs
        .clone()
}

fn members(list: &[&str]) -> UsedRefs {
    UsedRefs::Members(list.iter().map(|s| s.to_string()).collect())
}

#[test]
fn symbols_are_off_by_default() {
    let src = "import * as ns from './a'; export const x = ns.a;";
    let f = extract(
        Path::new("a.ts"),
        src,
        &ExtractOptions::default(),
        &Allocator::default(),
    );
    assert_eq!(f.exports[0].loc, None);
    assert_eq!(f.imports[0].refs, UsedRefs::Opaque);
}

#[test]
fn reexports_keep_the_name_they_read() {
    let f = facts(
        "a.ts",
        "export { a as b, c } from './x';\nexport * from './y';\nexport * as ns from './z';\nexport { default } from './d';\nexport { default as e } from './d';",
    );
    let b = export(&f, "b");
    assert_eq!(
        (b.kind, b.imported.as_deref()),
        (ExportKind::Reexport, Some("a"))
    );
    assert_eq!(export(&f, "c").imported.as_deref(), Some("c"));
    assert_eq!(export(&f, "default").imported.as_deref(), Some("default"));
    assert_eq!(export(&f, "e").imported.as_deref(), Some("default"));
    assert_eq!(export(&f, "*").kind, ExportKind::Star);
    assert_eq!(export(&f, "ns").kind, ExportKind::Namespace);
    assert_eq!(export(&f, "b").loc.unwrap().line, 1);
    assert_eq!(export(&f, "ns").loc.unwrap().line, 3);
}

#[test]
fn an_imported_binding_that_is_exported_is_a_reexport() {
    let f = facts(
        "a.ts",
        "import { a } from './x';\nimport d from './d';\nexport { a, d as e };\n",
    );
    let a = export(&f, "a");
    assert_eq!(
        (a.kind, a.from.as_deref(), a.imported.as_deref()),
        (ExportKind::Reexport, Some("./x"), Some("a"))
    );
    let e = export(&f, "e");
    assert_eq!(
        (e.from.as_deref(), e.imported.as_deref()),
        (Some("./d"), Some("default"))
    );
}

#[test]
fn member_reads_narrow_an_import() {
    let f = facts(
        "a.ts",
        "import * as ns from './x';\nimport { E } from './e';\nimport * as all from './y';\nimport * as unused from './z';\nconsole.log(ns.a, ns.b.c, E.X);\nuse(all);\ntype T = ns.Ty;",
    );
    assert_eq!(refs(&f, "ns"), members(&["Ty", "a", "b"]));
    assert_eq!(refs(&f, "E"), members(&["X"]));
    assert_eq!(refs(&f, "all"), UsedRefs::Opaque);
    assert_eq!(refs(&f, "unused"), members(&[]));
}

#[test]
fn any_other_use_makes_the_import_opaque() {
    let f = facts(
        "a.ts",
        "import * as ns from './x';\nimport { E } from './e';\nconst { a } = ns;\nconsole.log(E.X, Object.values(E));",
    );
    assert_eq!(refs(&f, "ns"), UsedRefs::Opaque);
    assert_eq!(refs(&f, "E"), UsedRefs::Opaque);
}

#[test]
fn a_reexported_namespace_import_is_opaque() {
    let f = facts("a.ts", "import * as ns from './x';\nexport { ns };");
    assert_eq!(refs(&f, "ns"), UsedRefs::Opaque);
}

#[test]
fn single_file_components_are_never_narrowed() {
    let f = facts(
        "c.vue",
        "<script setup lang=\"ts\">\nimport * as ns from './x';\nconst a = ns.a;\n</script>\n<template>{{ ns.b }}</template>",
    );
    assert_eq!(refs(&f, "ns"), UsedRefs::Opaque);
}

#[test]
fn exports_know_if_the_file_uses_them() {
    let f = facts(
        "a.ts",
        "export const used = 1;\nexport const alone = 2;\nexport function f() { return used; }\nexport { f as g };",
    );
    assert!(export(&f, "used").used_in_file);
    assert!(!export(&f, "alone").used_in_file);
    // a name in the export list is not a use
    assert!(!export(&f, "g").used_in_file);
}

#[test]
fn enum_and_namespace_members_are_listed() {
    let f = facts(
        "a.ts",
        "export enum E { A, B = 2, 'c-d' = 3 }\nenum Local { X }\nexport { Local };\nexport namespace N { export const a = 1; export function f() {} const hidden = 1; }\nexport declare const C: number;",
    );
    let names = |n: &str| -> Vec<String> {
        export(&f, n)
            .members
            .iter()
            .map(|m| m.name.clone())
            .collect()
    };
    assert_eq!(names("E"), ["A", "B", "c-d"]);
    assert_eq!(names("Local"), ["X"]);
    assert_eq!(names("N"), ["a", "f"]);
    assert!(names("C").is_empty());
    assert_eq!(export(&f, "E").members[1].loc.line, 1);
}

#[test]
fn exports_can_be_suppressed_one_by_one() {
    let f = facts(
        "a.ts",
        "// oxcruise-ignore-next-line no-unused-exports\nexport const a = 1;\nexport const b = 2; // oxcruise-ignore-line\nexport const c = 3;\nexport {\n  // oxcruise-ignore-next-line no-unused-exports -- why\n  d,\n  e,\n};\nconst d = 1, e = 2;",
    );
    assert_eq!(export(&f, "a").suppressed, ["no-unused-exports"]);
    assert_eq!(export(&f, "b").suppressed, ["*"]);
    assert!(export(&f, "c").suppressed.is_empty());
    assert_eq!(export(&f, "d").suppressed, ["no-unused-exports"]);
    assert!(export(&f, "e").suppressed.is_empty());
}

#[test]
fn one_binding_under_two_names_is_a_duplicate() {
    let f = facts(
        "a.ts",
        "const Foo = 1;\nexport default Foo;\nexport { Foo };\nexport { Foo as Bar };",
    );
    assert_eq!(export(&f, "default").duplicate_of, None);
    assert_eq!(export(&f, "Foo").duplicate_of.as_deref(), Some("default"));
    assert_eq!(export(&f, "Bar").duplicate_of.as_deref(), Some("default"));
}

#[test]
fn types_are_marked() {
    let f = facts(
        "a.ts",
        "export type T = 1;\nexport interface I {}\nexport const v = 1;",
    );
    assert!(export(&f, "T").is_type);
    assert!(export(&f, "I").is_type);
    assert!(!export(&f, "v").is_type);
}

/// Apply the fix of every export named in `names` to `src`.
fn fixed(src: &str, names: &[&str]) -> String {
    let f = facts("a.ts", src);
    let mut edits: Vec<(u32, u32)> = names.iter().filter_map(|n| export(&f, n).fix).collect();
    edits.sort();
    let mut out = src.to_string();
    for (s, e) in edits.into_iter().rev() {
        out.replace_range(s as usize..e as usize, "");
    }
    out
}

#[test]
fn removing_an_export_keeps_the_declaration() {
    assert_eq!(fixed("export const a = 1;\n", &["a"]), "const a = 1;\n");
    assert_eq!(
        fixed("export function f() {}\n", &["f"]),
        "function f() {}\n"
    );
    assert_eq!(fixed("export type T = 1;\n", &["T"]), "type T = 1;\n");
    assert_eq!(fixed("export enum E { A }\n", &["E"]), "enum E { A }\n");
    // one statement exporting two names cannot lose its keyword for one of them
    assert_eq!(
        fixed("export const a = 1, b = 2;\n", &["a", "b"]),
        "export const a = 1, b = 2;\n"
    );
}

#[test]
fn removing_a_listed_export_takes_its_comma() {
    let src = "const a = 1, b = 2, c = 3;\nexport { a, b as bee, c };\n";
    assert_eq!(
        fixed(src, &["a"]),
        "const a = 1, b = 2, c = 3;\nexport { b as bee, c };\n"
    );
    assert_eq!(
        fixed(src, &["bee"]),
        "const a = 1, b = 2, c = 3;\nexport { a, c };\n"
    );
    assert_eq!(
        fixed(src, &["c"]),
        "const a = 1, b = 2, c = 3;\nexport { a, b as bee };\n"
    );
    assert_eq!(
        fixed("const a = 1;\nexport { a };\n", &["a"]),
        "const a = 1;\n\n"
    );
    assert_eq!(
        fixed("export { x, y as z } from './m';\n", &["z"]),
        "export { x } from './m';\n"
    );
    assert_eq!(fixed("export * as ns from './m';\n", &["ns"]), "\n");
}

/// Apply every fix of the file `file` to `src`, returning the removed texts.
fn removed_texts(file: &str, src: &str) -> Vec<(String, String)> {
    let f = facts(file, src);
    f.exports
        .iter()
        .map(|e| {
            let (s, end) = e
                .fix
                .unwrap_or_else(|| panic!("no fix for `{}` in {file}", e.name));
            (e.name.clone(), src[s as usize..end as usize].to_string())
        })
        .collect()
}

#[test]
fn a_svelte_file_with_two_scripts_keeps_the_fixes_of_both() {
    let src = "<script context=\"module\">export const a = 1; export function b() {}</script>\n<script>let x = 1;</script>\n";
    let texts = removed_texts("C.svelte", src);
    assert_eq!(
        texts,
        [
            ("a".to_string(), "export ".to_string()),
            ("b".to_string(), "export ".to_string()),
        ]
    );
}

#[test]
fn a_vue_file_with_two_scripts_keeps_the_fixes_of_both() {
    let src = "<script lang=\"ts\">export interface P {}</script>\n<script setup lang=\"ts\">export const x = 1;</script>\n";
    let f = facts("C.vue", src);
    assert!(!f.exports.is_empty());
    let texts = removed_texts("C.vue", src);
    assert!(texts.iter().all(|(_, t)| t == "export "), "{texts:?}");
}

#[test]
fn imports_of_a_second_script_block_do_not_borrow_from_the_first() {
    let src = "<script context=\"module\">import { a } from './a'; export const m = a;</script>\n<script>import { b } from './b'; b.go();</script>\n";
    let f = facts("C.svelte", src);
    assert_eq!(f.imports.len(), 2);
    assert!(f.exports.iter().all(|e| e.fix.is_some()));
}
