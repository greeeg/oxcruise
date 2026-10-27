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
fn check_subcommand_is_the_default() {
    let a = run(&["packages", "-f", "text"]);
    let b = run(&["check", "packages", "-f", "text"]);
    assert_eq!(a.stdout, b.stdout);
}

#[test]
fn filter_flags() {
    let o = run(&[
        "packages",
        "-f",
        "text",
        "--exclude",
        "packages/lib",
        "--do-not-follow",
        "packages/app/src/util.ts",
    ]);
    assert!(!o.stdout.contains("packages/lib"), "{}", o.stdout);
    assert!(
        !o.stdout.contains("util.ts -> "),
        "do-not-follow keeps util.ts as a leaf:\n{}",
        o.stdout
    );
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
    let compat = run_in(errors.path(), &["--compat"]);
    assert_eq!(
        compat.result,
        CliRunResult::FoundErrors {
            errors: 1,
            compat: true
        }
    );
    assert_eq!(compat.result.code(), 1);
}

#[test]
fn config_discovery_and_no_config() {
    let d = project(&[
        ("src/a.ts", "import './a';\n"),
        (
            ".oxcruiserc.json",
            r#"{ "entries": ["src"], "rules": { "no-circular": "off" } }"#,
        ),
    ]);
    assert_eq!(run_in(d.path(), &[]).result, CliRunResult::Success);
    // --no-config falls back to the recommended preset, which flags the self-import
    let o = run_in(d.path(), &["src", "--no-config"]);
    assert_eq!(o.result.code(), 1);
    assert!(o.stdout.contains("no-circular"), "{}", o.stdout);
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
fn dependency_cruiser_config_is_imported_with_a_note() {
    let d = project(&[
        ("src/a.ts", "import './b';\n"),
        ("src/b.ts", "import './a';\n"),
        (
            ".dependency-cruiser.json",
            r#"{ "forbidden": [{ "name": "no-circular", "severity": "error", "from": {}, "to": { "circular": true } }],
                 "options": { "includeOnly": "^src" } }"#,
        ),
    ]);
    let o = run_in(d.path(), &["src", "-f", "text"]);
    assert_eq!(o.result.code(), 1);
    assert!(
        o.stderr.contains("dependency-cruiser importer"),
        "{}",
        o.stderr
    );
    assert!(o.stderr.contains("oxcruise migrate"), "{}", o.stderr);
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
fn output_file_and_fmt_round_trip() {
    let d = project(&[
        ("src/a.ts", "import './b';\n"),
        ("src/b.ts", "import './a';\n"),
        (
            ".oxcruiserc.json",
            r#"{ "entries": ["src"], "rules": { "no-circular": "warn" } }"#,
        ),
    ]);
    let p = d.path();
    assert_eq!(
        run_in(p, &["-f", "json", "-o", "result.json"]).result,
        CliRunResult::Success
    );
    assert!(p.join("result.json").is_file());
    let live = run_in(p, &["-f", "dot"]);
    let fmt = run_in(p, &["fmt", "result.json", "-f", "dot"]);
    assert_eq!(fmt.result, CliRunResult::Success);
    assert_eq!(live.stdout, fmt.stdout);

    std::fs::write(p.join("junk.json"), "{}").unwrap();
    let bad = run_in(p, &["fmt", "junk.json"]);
    assert_eq!(bad.result, CliRunResult::InvalidOptions);
    assert!(
        bad.stderr.contains("not an oxcruise `json` result"),
        "{}",
        bad.stderr
    );
}

#[test]
fn migrate_writes_a_typescript_config() {
    let d = project(&[(
        ".dependency-cruiser.json",
        r#"{
            "forbidden": [
              { "name": "no-circular", "severity": "warn", "from": {}, "to": { "circular": true } },
              { "name": "needs-license", "severity": "error", "from": {}, "to": { "license": "GPL" } }
            ],
            "options": { "doNotFollow": { "path": "node_modules" }, "webpackConfig": { "fileName": "webpack.config.js" } }
          }"#,
    )]);
    let p = d.path();
    let o = run_in(p, &["migrate"]);
    assert_eq!(o.result, CliRunResult::Success, "{}", o.stderr);
    let written = std::fs::read_to_string(p.join("oxcruise.config.ts")).unwrap();
    insta::assert_snapshot!("migrate_ts", written);
    insta::assert_snapshot!("migrate_stderr", o.stderr);

    // refuses to clobber, --force overwrites, --json writes JSON with a $schema
    assert_eq!(run_in(p, &["migrate"]).result, CliRunResult::InvalidOptions);
    assert_eq!(
        run_in(p, &["migrate", "--force"]).result,
        CliRunResult::Success
    );
    assert_eq!(
        run_in(p, &["migrate", "--json"]).result,
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

    let none = run_in(tempfile::tempdir().unwrap().path(), &["migrate"]);
    assert_eq!(none.result, CliRunResult::InvalidOptions);
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
