use std::path::{Path, PathBuf};

use oxcruise::{CliRunResult, CliRunner};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/monorepo")
        .canonicalize()
        .unwrap()
}

struct Out {
    result: CliRunResult,
    stdout: String,
    stderr: String,
}

fn run_in(cwd: &Path, args: &[&str]) -> Out {
    let (mut out, mut err) = (vec![], vec![]);
    let result = CliRunner::new(args.iter().map(|s| s.to_string()).collect())
        .with_cwd(cwd.to_path_buf())
        .run(&mut out, &mut err);
    // Replace the canonical path first: on macOS the raw temp dir (`/var/...`) is a
    // suffix of the canonical one (`/private/var/...`), and output may use either.
    let canonical = cwd.canonicalize().unwrap();
    let scrub = |b: Vec<u8>| {
        String::from_utf8(b)
            .unwrap()
            .replace(canonical.to_str().unwrap(), "<cwd>")
            .replace(cwd.to_str().unwrap(), "<cwd>")
            // Windows joins with `\`; snapshots are recorded with `/`.
            .replace("<cwd>\\", "<cwd>/")
            .lines()
            .filter(|l| !l.starts_with("finished in"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    Out {
        result,
        stdout: scrub(out),
        stderr: scrub(err),
    }
}

fn run(args: &[&str]) -> Out {
    run_in(&fixture(), args)
}

/// Snapshot name derived from the arguments, e.g. `packages_-f_github`.
fn name(args: &[&str]) -> String {
    args.join("_").replace(['/', '*', ' '], "-")
}

fn snap(args: &[&str]) -> Out {
    let o = run(args);
    insta::assert_snapshot!(
        name(args),
        format!(
            "exit: {}\n--- stdout\n{}\n--- stderr\n{}",
            o.result.code(),
            o.stdout,
            o.stderr
        )
    );
    o
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

#[test]
fn default_check_on_the_monorepo() {
    let o = snap(&["packages"]);
    assert_eq!(o.result.code(), 1, "errors exit 1");
}

#[test]
fn formats() {
    for f in ["github", "text", "mermaid"] {
        snap(&["packages", "-f", f]);
    }
}

#[test]
fn subcommand_names_are_just_paths() {
    // there are no subcommands: `check` and `migrate` are paths that do not exist
    for word in ["check", "migrate"] {
        let o = run(&[word]);
        assert_eq!(o.result, CliRunResult::InvalidOptions, "{word}");
    }
}

#[test]
fn lsp_refuses_check_flags_but_takes_config_and_threads() {
    for flags in [
        &["--lsp", "-f", "json"][..],
        &["--lsp", "src"],
        &["--lsp", "--init"],
    ] {
        let o = run(flags);
        assert_eq!(o.result, CliRunResult::InvalidOptions, "{flags:?}");
        assert!(o.stderr.contains("cannot be used with"), "{}", o.stderr);
    }
    // the flags it does take parse (and fail only for lack of a client)
    let o = run(&["--lsp", "--help"]);
    assert!(o.stdout.contains("--lsp"), "{}", o.stdout);
}

#[test]
fn filter_flags() {
    let o = run(&["packages", "-f", "text", "--ignore-pattern", "packages/lib"]);
    assert!(!o.stdout.contains("packages/lib"), "{}", o.stdout);
    let shallow = run(&[
        "packages/app/src/index.ts",
        "-f",
        "text",
        "--max-depth",
        "1",
    ]);
    assert!(
        shallow
            .stdout
            .contains("index.ts -> packages/app/src/cycle-a.ts")
    );
    assert!(
        !shallow.stdout.contains("cycle-a.ts -> "),
        "{}",
        shallow.stdout
    );
}

#[test]
fn do_not_follow_keeps_the_module_but_not_its_imports() {
    let d = project(&[
        ("src/a.ts", "import './gen';\n"),
        ("src/gen.ts", "import './b';\n"),
        ("src/b.ts", "export {};\n"),
        (
            ".oxcruiserc.json",
            r#"{ "entries": ["src/a.ts"], "doNotFollow": ["src/gen.ts"] }"#,
        ),
    ]);
    let o = run_in(d.path(), &["-f", "text"]);
    assert!(o.stdout.contains("src/a.ts -> src/gen.ts"), "{}", o.stdout);
    assert!(!o.stdout.contains("gen.ts -> "), "{}", o.stdout);
}

#[test]
fn exit_codes() {
    let warn_only = project(&[
        ("src/a.ts", "import './b';\n"),
        ("src/b.ts", "export {};\n"),
        ("src/lonely.ts", "export {};\n"),
        (
            ".oxcruiserc.json",
            r#"{ "entries": ["src"], "rules": { "no-orphans": "warn" } }"#,
        ),
    ]);
    let p = warn_only.path();
    assert_eq!(
        run_in(p, &[]).result,
        CliRunResult::Success,
        "warnings alone pass"
    );
    assert!(matches!(
        run_in(p, &["--deny-warnings"]).result,
        CliRunResult::NoWarningsAllowed { warnings: 1 }
    ));
    assert!(matches!(
        run_in(p, &["--max-warnings", "0"]).result,
        CliRunResult::MaxWarningsExceeded {
            warnings: 1,
            max: 0
        }
    ));
    assert_eq!(
        run_in(p, &["--max-warnings", "1"]).result,
        CliRunResult::Success
    );

    let errors = project(&[
        ("src/a.ts", "import './a';\n"),
        (
            ".oxcruiserc.json",
            r#"{ "entries": ["src"], "rules": { "no-circular": "error" } }"#,
        ),
    ]);
    let plain = run_in(errors.path(), &[]);
    assert_eq!(plain.result.code(), 1);
    assert_eq!(plain.result, CliRunResult::FoundErrors { errors: 1 });
}

#[test]
fn removed_flags_and_formats_are_rejected() {
    for args in [
        &["--compat"][..],
        &["--include-only", "src"],
        &["--do-not-follow", "src"],
        &["--exclude", "src"],
        &["fmt", "result.json"],
        &["-f", "err"],
        &["-f", "err-long"],
        &["-f", "json-depcruise"],
        &["-o", "result.json"],
        &["--output", "result.json"],
        &["--color", "never"],
        &["--no-config"],
        &["--force"],
        &["migrate", "--json"],
    ] {
        assert_eq!(run(args).result, CliRunResult::InvalidOptions, "{args:?}");
    }
}

#[test]
fn config_discovery() {
    let d = project(&[
        ("src/a.ts", "import './a';\n"),
        (
            ".oxcruiserc.json",
            r#"{ "entries": ["src"], "rules": { "no-circular": "off" } }"#,
        ),
    ]);
    assert_eq!(run_in(d.path(), &[]).result, CliRunResult::Success);
    let explicit = run_in(d.path(), &["--config", ".oxcruiserc.json"]);
    assert_eq!(explicit.result, CliRunResult::Success);
    let missing = run_in(d.path(), &["--config", "nope.json"]);
    assert_eq!(missing.result, CliRunResult::InvalidOptions);
    assert!(
        missing.stderr.contains("does not exist"),
        "{}",
        missing.stderr
    );
}

#[test]
fn config_errors_are_reported_together() {
    let d = project(&[
        ("src/a.ts", "export {};\n"),
        (
            ".oxcruiserc.json",
            r#"{ "entries": ["src"], "rules": { "no-circluar": "error", "layers": ["error", { "allow": { "x": [] } }] } }"#,
        ),
    ]);
    let o = run_in(d.path(), &[]);
    assert_eq!(o.result, CliRunResult::InvalidOptions);
    insta::assert_snapshot!("config_errors", o.stderr);
}

#[test]
fn dependency_cruiser_config_is_not_discovered() {
    // only `oxcruise --migrate` reads these; a check runs with the defaults and says nothing
    let d = project(&[
        ("src/a.ts", "import './b';\n"),
        ("src/b.ts", "export {};\n"),
        (
            ".dependency-cruiser.json",
            r#"{ "forbidden": [{ "name": "no-b", "severity": "error", "from": {}, "to": { "path": "b" } }] }"#,
        ),
    ]);
    let o = run_in(d.path(), &["src", "-f", "text"]);
    assert_eq!(o.result, CliRunResult::Success, "{}", o.stderr);
    assert!(o.stderr.is_empty(), "{}", o.stderr);
    // ... and an explicit --config still goes through the oxcruise loader
    let explicit = run_in(d.path(), &["src", "-c", ".dependency-cruiser.json"]);
    assert_eq!(explicit.result, CliRunResult::InvalidOptions);
    assert!(
        explicit.stderr.contains("unknown field"),
        "{}",
        explicit.stderr
    );
}

#[test]
fn dead_forbid_rules_warn_on_stderr() {
    let d = project(&[
        ("src/a.ts", "export {};\n"),
        (
            ".oxcruiserc.json",
            r#"{ "entries": ["src"], "forbid": [{ "name": "typo", "from": "scr/**", "to": "x" }] }"#,
        ),
    ]);
    let o = run_in(d.path(), &[]);
    assert_eq!(o.result, CliRunResult::Success);
    assert!(
        o.stderr
            .contains("warning: rule `typo`: its `from` matched no module"),
        "{}",
        o.stderr
    );
    assert!(!o.stdout.contains("typo"), "{}", o.stdout);
}

#[test]
fn typescript_config_is_evaluated_by_node() {
    if std::process::Command::new("node")
        .arg("--version")
        .output()
        .is_err()
    {
        return;
    }
    let d = project(&[
        ("src/a.ts", "import './a';\n"),
        (
            "oxcruise.config.ts",
            "const entries: string[] = ['src'];\nexport default { entries, rules: { 'no-circular': 'warn' as const } };\n",
        ),
    ]);
    let o = run_in(d.path(), &[]);
    assert_eq!(
        o.result,
        CliRunResult::Success,
        "{}\n{}",
        o.stdout,
        o.stderr
    );
    assert!(o.stdout.contains("warn[no-circular]"), "{}", o.stdout);
}

#[test]
fn js_plugins_need_the_npm_package() {
    let d = project(&[
        ("src/a.ts", "export {};\n"),
        (
            ".oxcruiserc.json",
            r#"{ "entries": ["src"], "jsPlugins": ["./plugin.ts"] }"#,
        ),
    ]);
    let o = run_in(d.path(), &[]);
    assert_eq!(o.result, CliRunResult::InvalidOptions);
    assert!(o.stderr.contains("need the npm package"), "{}", o.stderr);
}

#[test]
fn migrate_writes_a_typescript_config() {
    let d = project(&[(
        ".dependency-cruiser.json",
        r#"{
            "forbidden": [
              { "name": "no-circular", "severity": "warn", "from": {}, "to": { "circular": true } },
              { "name": "needs-license", "severity": "error", "from": {}, "to": { "license": "GPL" } },
              { "name": "feature-isolation", "severity": "error", "comment": "features stay apart",
                "from": { "path": "^src/features/([^/]+)/" },
                "to": { "path": "^src/features/", "pathNot": "^src/features/$1/" } },
              { "name": "not-in-prod", "severity": "info", "from": { "path": "^src" }, "to": { "path": "[.]spec[.]ts$" } }
            ],
            "options": { "doNotFollow": { "path": "node_modules" }, "webpackConfig": { "fileName": "webpack.config.js" } }
          }"#,
    )]);
    let p = d.path();
    let o = run_in(p, &["--migrate"]);
    assert_eq!(o.result, CliRunResult::Success, "{}", o.stderr);
    let written = std::fs::read_to_string(p.join("oxcruise.config.ts")).unwrap();
    insta::assert_snapshot!("migrate_ts", written);
    insta::assert_snapshot!("migrate_stderr", o.stderr);

    // refuses to clobber; -c with a .json name writes JSON with a $schema
    let again = run_in(p, &["--migrate"]);
    assert_eq!(again.result, CliRunResult::InvalidOptions);
    assert!(again.stderr.contains("already exists"), "{}", again.stderr);
    assert_eq!(
        run_in(p, &["--migrate", "-c", ".oxcruiserc.json"]).result,
        CliRunResult::Success
    );
    let json = std::fs::read_to_string(p.join(".oxcruiserc.json")).unwrap();
    assert!(json.contains("\"$schema\""), "{json}");
    // the migrated JSON config loads and runs
    std::fs::remove_file(p.join("oxcruise.config.ts")).unwrap();
    std::fs::remove_file(p.join(".dependency-cruiser.json")).unwrap();
    std::fs::create_dir_all(p.join("src")).unwrap();
    std::fs::write(p.join("src/a.ts"), "export {};\n").unwrap();
    let again = run_in(p, &["src"]);
    assert!(
        matches!(
            again.result,
            CliRunResult::Success | CliRunResult::FoundErrors { .. }
        ),
        "{}",
        again.stderr
    );

    let none = run_in(tempfile::tempdir().unwrap().path(), &["--migrate"]);
    assert_eq!(none.result, CliRunResult::InvalidOptions);
    assert!(none.stderr.contains("--migrate=FILE"), "{}", none.stderr);

    // an explicit source, anywhere
    let q = project(&[(
        "legacy/dc.json",
        r#"{ "forbidden": [{ "name": "no-b", "severity": "error", "from": {}, "to": { "path": "b" } }] }"#,
    )]);
    let o = run_in(q.path(), &["--migrate=legacy/dc.json"]);
    assert_eq!(o.result, CliRunResult::Success, "{}", o.stderr);
    assert!(q.path().join("oxcruise.config.ts").is_file());
}

#[test]
fn init_writes_a_config_that_loads() {
    let d = project(&[("src/a.ts", "export {};\n")]);
    let p = d.path();
    let o = run_in(p, &["--init"]);
    assert_eq!(o.result, CliRunResult::Success, "{}", o.stderr);
    assert!(o.stdout.starts_with("wrote "), "{}", o.stdout);
    let ts = std::fs::read_to_string(p.join("oxcruise.config.ts")).unwrap();
    insta::assert_snapshot!("init_ts", ts);

    let again = run_in(p, &["--init"]);
    assert_eq!(again.result, CliRunResult::InvalidOptions);
    assert!(again.stderr.contains("already exists"), "{}", again.stderr);

    let o = run_in(p, &["--init", "-c", ".oxcruiserc.json"]);
    assert_eq!(o.result, CliRunResult::Success, "{}", o.stderr);
    let json = std::fs::read_to_string(p.join(".oxcruiserc.json")).unwrap();
    insta::assert_snapshot!("init_json", json);

    // the written JSON config is picked up and runs (the .ts one needs Node, so go through -c)
    let ran = run_in(p, &["src", "-c", ".oxcruiserc.json"]);
    assert_eq!(ran.result, CliRunResult::Success, "{}", ran.stderr);
}

#[test]
fn rules_table_and_json() {
    let table = run(&["--rules"]);
    assert_eq!(table.result, CliRunResult::Success);
    insta::assert_snapshot!("rules_table", table.stdout);

    let json = run(&["--rules", "-f", "json"]);
    assert_eq!(json.result, CliRunResult::Success);
    let rules: serde_json::Value = serde_json::from_str(&json.stdout).unwrap();
    assert!(
        rules
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["name"] == "no-circular" && r["category"] == "correctness")
    );

    let bad = run(&["--rules", "-f", "dot"]);
    assert_eq!(bad.result, CliRunResult::InvalidOptions);
}

#[test]
fn print_config_fills_categories() {
    let d = project(&[(
        ".oxcruiserc.json",
        r#"{ "entries": ["src"], "categories": { "style": "warn" } }"#,
    )]);
    let o = run_in(d.path(), &["--print-config"]);
    assert_eq!(o.result, CliRunResult::Success, "{}", o.stderr);
    let v: serde_json::Value = serde_json::from_str(&o.stdout).unwrap();
    assert_eq!(v["entries"], serde_json::json!(["src"]));
    assert_eq!(
        v["categories"],
        serde_json::json!({
            "correctness": "error", "suspicious": "warn", "style": "warn",
            "restriction": "off", "nursery": "off"
        })
    );
}

#[test]
fn quiet_hides_warnings_but_keeps_limits() {
    let d = project(&[
        ("src/a.ts", "import './b';\n"),
        ("src/b.ts", "export {};\n"),
        ("src/lonely.ts", "export {};\n"),
        (
            ".oxcruiserc.json",
            r#"{ "entries": ["src"], "rules": { "no-orphans": "warn" } }"#,
        ),
    ]);
    let p = d.path();
    let loud = run_in(p, &[]);
    assert!(loud.stdout.contains("no-orphans"), "{}", loud.stdout);
    let quiet = run_in(p, &["--quiet"]);
    assert_eq!(quiet.result, CliRunResult::Success);
    assert!(!quiet.stdout.contains("no-orphans"), "{}", quiet.stdout);
    // the warning is still counted
    assert!(matches!(
        run_in(p, &["--quiet", "--deny-warnings"]).result,
        CliRunResult::NoWarningsAllowed { warnings: 1 }
    ));
    assert!(matches!(
        run_in(p, &["--quiet", "--max-warnings", "0"]).result,
        CliRunResult::MaxWarningsExceeded { .. }
    ));
}

#[test]
fn modes_conflict() {
    for args in [
        &["--init", "--rules"][..],
        &["--init", "--migrate"],
        &["--rules", "--print-config"],
        &["--init", "src"],
        &["--migrate", "--quiet"],
    ] {
        let o = run(args);
        assert_eq!(o.result.code(), 2, "{args:?}");
        assert_eq!(o.result, CliRunResult::InvalidOptions, "{args:?}");
    }
}

#[test]
fn help_lists_the_sections() {
    let o = run(&["--help"]);
    insta::assert_snapshot!("help", o.stdout);
}

#[test]
fn help_and_version_succeed() {
    for a in ["--help", "--version"] {
        let o = run(&[a]);
        assert_eq!(o.result, CliRunResult::Success);
        assert!(o.stdout.contains("oxcruise"), "{}", o.stdout);
    }
    assert_eq!(run(&["--nope"]).result, CliRunResult::InvalidOptions);
}
