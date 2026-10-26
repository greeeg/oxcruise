use std::path::Path;

use oxc_allocator::Allocator;
use oxcruise_extract::{ExtractOptions, extract};

fn opts() -> ExtractOptions {
    ExtractOptions {
        exotic_require_strings: vec!["need".into(), "window.require".into()],
        calls: vec!["jest.mock".into(), "require.resolve".into()],
        jsdoc: true,
        skip_triple_slash: false,
    }
}

fn run(name: &str) -> oxcruise_extract::FileFacts {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let source = std::fs::read_to_string(dir.join(name)).unwrap();
    let alloc = Allocator::default();
    extract(Path::new(name), &source, &opts(), &alloc)
}

macro_rules! snapshot_fixtures {
    ($($test:ident => $file:literal),* $(,)?) => {$(
        #[test]
        fn $test() {
            insta::assert_json_snapshot!($file, run($file));
        }
    )*};
}

snapshot_fixtures! {
    es6_import_export => "es6-import-export.ts",
    type_only => "type-only.ts",
    dynamic_import => "dynamic-import.js",
    require_cjs => "require.cjs",
    exotic_require => "exotic.js",
    import_equals => "import-equals.ts",
    import_type => "import-type.ts",
    triple_slash => "triple-slash.ts",
    jsdoc => "jsdoc.js",
    amd => "amd.js",
    url_worker => "url-worker.js",
    suppression => "suppression.ts",
    suppress_file => "suppress-file.ts",
    vue => "component.vue",
    svelte => "component.svelte",
    astro => "page.astro",
    mts => "module.mts",
    cts => "module.cts",
    parse_error => "parse-error.ts",
    jsx => "jsx-file.jsx",
}
