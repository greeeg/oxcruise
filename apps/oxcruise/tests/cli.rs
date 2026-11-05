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
    for f in ["github", "text"] {
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
            r#"{ "entries": ["src"], "rules": { "orphan-modules": "warn" } }"#,
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
            r#"{ "entries": ["src"], "rules": { "circular-dependencies": "error" } }"#,
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
            r#"{ "entries": ["src"], "rules": { "circular-dependencies": "off" } }"#,
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
            r#"{ "entries": ["src"], "rules": { "circular-dependancies": "error", "layers": ["error", { "allow": { "x": [] } }] } }"#,
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
            "const entries: string[] = ['src'];\nexport default { entries, rules: { 'circular-dependencies': 'warn' as const } };\n",
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
    assert!(
        o.stdout.contains("warn[circular-dependencies]"),
        "{}",
        o.stdout
    );
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
            .any(|r| r["name"] == "circular-dependencies" && r["category"] == "correctness")
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
            r#"{ "entries": ["src"], "rules": { "orphan-modules": "warn" } }"#,
        ),
    ]);
    let p = d.path();
    let loud = run_in(p, &[]);
    assert!(loud.stdout.contains("orphan-modules"), "{}", loud.stdout);
    let quiet = run_in(p, &["--quiet"]);
    assert_eq!(quiet.result, CliRunResult::Success);
    assert!(!quiet.stdout.contains("orphan-modules"), "{}", quiet.stdout);
    // the warning is still counted
    assert!(matches!(
        run_in(p, &["--quiet", "--deny-warnings"]).result,
        CliRunResult::NoWarningsAllowed { warnings: 1 }
    ));
    assert!(matches!(
        run_in(p, &["--quiet", "--max-warnings", "0"]).result,
        CliRunResult::MaxWarningsExceeded { .. }
    ));
    // and so does the summary of the report
    let json = |args: &[&str]| -> serde_json::Value {
        serde_json::from_str(&run_in(p, args).stdout).unwrap()
    };
    let hidden = json(&["--quiet", "-f", "json"]);
    assert_eq!(hidden["violations"], serde_json::json!([]));
    assert_eq!(hidden["summary"]["warn"], 1);
    assert_eq!(hidden["summary"], json(&["-f", "json"])["summary"]);
}

#[test]
fn json_circular_does_not_depend_on_the_rule() {
    let d = project(&[
        ("src/a.ts", "import './b';\nimport './c';\n"),
        ("src/b.ts", "import './a';\n"),
        ("src/c.ts", "export {};\n"),
        (
            ".oxcruiserc.json",
            r#"{ "entries": ["src"], "rules": { "circular-dependencies": "off" } }"#,
        ),
    ]);
    let json: serde_json::Value =
        serde_json::from_str(&run_in(d.path(), &["-f", "json"]).stdout).unwrap();
    let circular = |module: &str, to: &str| {
        let m = json["modules"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["path"] == module)
            .unwrap();
        let dep = m["dependencies"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["to"] == to)
            .unwrap();
        (
            dep["circular"].as_bool().unwrap(),
            dep.get("cycle").is_none(),
        )
    };
    // on a cycle although no rule reports it, so there is no `cycle` to show
    assert_eq!(circular("src/a.ts", "src/b.ts"), (true, true));
    assert_eq!(circular("src/b.ts", "src/a.ts"), (true, true));
    assert_eq!(circular("src/a.ts", "src/c.ts"), (false, true));
    // module kinds use the names of the config
    let kinds: Vec<&str> = json["modules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["kind"].as_str().unwrap())
        .collect();
    assert!(kinds.iter().all(|k| *k == "local"), "{kinds:?}");
}

#[test]
fn modes_conflict() {
    for args in [
        &["--init", "--rules"][..],
        &["--init", "--migrate"],
        &["--rules", "--print-config"],
        &["--init", "src"],
        &["--migrate", "--quiet"],
        &["--rules", "--fix"],
        &["--rules", "--threads", "2"],
        &["--print-config", "--production"],
        &["--print-config", "src"],
        &["--print-config", "-f", "json"],
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

fn unused_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/unused")
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn report(o: &Out) -> String {
    format!(
        "exit: {}\n--- stdout\n{}\n--- stderr\n{}",
        o.result.code(),
        o.stdout,
        o.stderr
    )
}

#[test]
fn unused_rules_report_exports_dependencies_and_files() {
    let o = run_in(&unused_fixture(), &["-f", "github"]);
    insta::assert_snapshot!("unused_github", report(&o));
}

#[test]
fn production_leaves_tests_out() {
    let o = run_in(&unused_fixture(), &["-f", "github", "--production"]);
    insta::assert_snapshot!("unused_production", report(&o));
}

#[test]
fn fix_removes_unused_exports_and_dependencies() {
    let dir = std::env::temp_dir().join(format!("oxcruise-unused-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy_dir(&unused_fixture(), &dir);

    let first = run_in(&dir, &["-f", "github", "--fix"]);
    let read = |p: &str| std::fs::read_to_string(dir.join(p)).unwrap();
    insta::assert_snapshot!("unused_fix_report", report(&first));
    insta::assert_snapshot!("unused_fix_util", read("src/util.ts"));
    insta::assert_snapshot!("unused_fix_api", read("src/api.ts"));
    insta::assert_snapshot!("unused_fix_package", read("package.json"));

    // a second pass has nothing left to repair: neighbouring unused dependencies go in one run
    let second = run_in(&dir, &["-f", "github", "--fix"]);
    insta::assert_snapshot!("unused_fix_second", report(&second));
    let third = run_in(&dir, &["-f", "github"]);
    insta::assert_snapshot!("unused_fix_third", report(&third));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fix_refuses_the_file_writing_modes() {
    let o = run_in(&unused_fixture(), &["--init", "--fix"]);
    assert_eq!(o.result.code(), 2, "{}", o.stderr);
}

/// `used` of every export of the json report, as `module:name -> used`.
fn used_exports(json: &str) -> Vec<(String, bool)> {
    let v: serde_json::Value = serde_json::from_str(json).unwrap();
    let mut out = vec![];
    for m in v["modules"].as_array().unwrap() {
        for x in m["exports"].as_array().into_iter().flatten() {
            out.push((
                format!(
                    "{}:{}",
                    m["path"].as_str().unwrap(),
                    x["name"].as_str().unwrap()
                ),
                x["used"].as_bool().unwrap(),
            ));
        }
    }
    out
}

#[test]
fn export_usage_does_not_depend_on_the_rules_that_are_on() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/unused-entry");
    let without = run_in(&dir, &["--format", "json", "--symbols", "src"]);
    let with = run_in(
        &dir,
        &[
            "--format",
            "json",
            "--symbols",
            "--config",
            "with-rule.json",
            "src",
        ],
    );
    let (a, b) = (used_exports(&without.stdout), used_exports(&with.stdout));
    assert!(!a.is_empty(), "{}", without.stdout);
    // `main` makes the index public, so `b` is used though only a test imports `a`
    assert!(a.iter().all(|(_, used)| *used), "{a:?}");
    assert_eq!(a, b);
}

#[test]
fn symbols_are_in_the_json_only_when_asked_for() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/unused-entry");
    let plain = run_in(&dir, &["--format", "json", "src"]);
    assert!(!plain.stdout.contains("\"exports\""), "{}", plain.stdout);
    assert!(!plain.stdout.contains("\"names\""), "{}", plain.stdout);
    let asked = run_in(&dir, &["--format", "json", "--symbols", "src"]);
    assert!(asked.stdout.contains("\"exports\""), "{}", asked.stdout);
    assert!(asked.stdout.contains("\"names\""), "{}", asked.stdout);
}

#[test]
fn the_symbols_key_of_the_config_turns_them_on() {
    let d = project(&[
        ("src/a.ts", "export const a = 1;\n"),
        (".oxcruiserc.json", r#"{ "symbols": true }"#),
    ]);
    let o = run_in(d.path(), &["--format", "json", "src"]);
    assert!(o.stdout.contains("\"exports\""), "{}", o.stdout);
}

#[test]
fn fix_removes_neighbouring_unused_dependencies_in_one_run() {
    let d = project(&[
        ("src/index.ts", "import \"used\";\n"),
        (
            "package.json",
            "{\n  \"name\": \"p\",\n  \"main\": \"src/index.ts\",\n  \"dependencies\": {\n    \"used\": \"1\",\n    \"x\": \"1\",\n    \"y\": \"1\",\n    \"z\": \"1\"\n  }\n}\n",
        ),
        (
            ".oxcruiserc.json",
            r#"{ "categories": { "correctness": "off", "suspicious": "off" }, "rules": { "unused-packages": "error" } }"#,
        ),
    ]);
    let first = run_in(d.path(), &["-f", "github", "--fix", "src"]);
    assert_eq!(first.result.code(), 0, "{}\n{}", first.stdout, first.stderr);
    let text = std::fs::read_to_string(d.path().join("package.json")).unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).expect("valid JSON after --fix");
    assert_eq!(
        json["dependencies"],
        serde_json::json!({ "used": "1" }),
        "{text}"
    );
    // nothing is left for a second run
    let second = run_in(d.path(), &["-f", "github", "src"]);
    assert_eq!(second.result.code(), 0, "{}", second.stdout);
    assert!(
        !second.stdout.contains("unused-packages"),
        "{}",
        second.stdout
    );
}

#[test]
fn unused_modules_warns_when_it_finds_no_entry_point() {
    let d = project(&[
        ("package.json", r#"{ "name": "p" }"#),
        ("src/a.ts", "import \"./b\";\n"),
        ("src/b.ts", "export const b = 1;\n"),
        (
            ".oxcruiserc.json",
            r#"{ "categories": { "correctness": "off", "suspicious": "off" }, "rules": { "unused-modules": "error" } }"#,
        ),
    ]);
    let o = run_in(d.path(), &["-f", "github", "src"]);
    insta::assert_snapshot!("unused_modules_without_entry", report(&o));
    // an entry point makes the warning go away
    std::fs::write(
        d.path().join("package.json"),
        r#"{ "name": "p", "main": "src/a.ts" }"#,
    )
    .unwrap();
    let o = run_in(d.path(), &["-f", "github", "src"]);
    assert!(!o.stderr.contains("no entry point"), "{}", o.stderr);
}

#[test]
fn visualize_writes_the_site_and_does_not_gate() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("site");
    let arg = format!("--visualize={}", target.display());
    let o = run(&["packages", &arg]);
    // the monorepo has errors, but the mode shows the graph and exits 0
    assert_eq!(o.result, CliRunResult::Success, "{}", o.stderr);
    assert!(o.stderr.contains("wrote "), "{}", o.stderr);
    assert!(o.stdout.is_empty(), "{}", o.stdout);
    for f in ["index.html", "app.js", "app.css", "data.js"] {
        assert!(target.join(f).is_file(), "{f}");
    }
    let data = std::fs::read_to_string(target.join("data.js")).unwrap();
    let json = data
        .strip_prefix("globalThis.OXCRUISE_DATA = JSON.parse('")
        .and_then(|d| d.trim_end().strip_suffix("');"))
        .expect("data.js shape");
    let v: serde_json::Value = serde_json::from_str(&json.replace("\\\\", "\\")).unwrap();
    assert_eq!(v["version"], 1);
    assert!(v["modules"]["name"].as_array().unwrap().len() > 5);
}

#[test]
fn visualize_defaults_to_oxcruise_report_in_the_cwd() {
    let d = project(&[("src/a.ts", "import './b'\n"), ("src/b.ts", "export {}\n")]);
    let o = run_in(d.path(), &["src", "--visualize"]);
    assert_eq!(o.result, CliRunResult::Success, "{}", o.stderr);
    assert!(d.path().join("oxcruise-report/index.html").is_file());
}

#[test]
fn visualize_refuses_check_only_flags() {
    for flag in [
        &["-f", "json"][..],
        &["--fix"],
        &["--quiet"],
        &["--deny-warnings"],
    ] {
        let mut args = vec!["--visualize"];
        args.extend_from_slice(flag);
        assert_eq!(run(&args).result, CliRunResult::InvalidOptions, "{flag:?}");
    }
    for f in ["dot", "mermaid"] {
        assert_eq!(
            run(&["packages", "-f", f]).result,
            CliRunResult::InvalidOptions,
            "{f}"
        );
    }
}
