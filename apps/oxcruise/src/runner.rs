use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use clap::Parser;
use oxcruise_config::{Category, JsConfigLoader, NodeSubprocessLoader};
use oxcruise_core::Severity;
use oxcruise_graph::cruise;
use oxcruise_language_server::{AnalyzeOutcome, Connection, LspAnalysis};
use oxcruise_report::{
    CruiseResult, ExternalReporters, OutputFormat, ReporterOptions, render, violation_outs,
};
use oxcruise_rules::{Engine, ExternalRules, PluginRef, builtin_rules};

use crate::args::{Cli, Mode};
use crate::result::CliRunResult;
use crate::{config_io, init, migrate, options, rules_list};

/// What a run found, before it is rendered.
pub struct Analysis {
    pub cwd: PathBuf,
    pub graph: oxcruise_core::Graph,
    pub workspace: Vec<oxcruise_resolve::WorkspacePackage>,
    /// Sorted, with suppressed violations removed.
    pub violations: Vec<oxcruise_core::Violation>,
    pub suppressed: usize,
    pub errors: usize,
    pub warnings: usize,
}

pub struct CliRunner {
    args: Vec<String>,
    js_loader: Arc<dyn JsConfigLoader>,
    external_rules: Option<Arc<dyn ExternalRules>>,
    external_reporters: Option<Arc<dyn ExternalReporters>>,
    cwd: Option<PathBuf>,
    stdout_is_tty: bool,
}

impl CliRunner {
    pub fn new(args: Vec<String>) -> Self {
        Self {
            args,
            js_loader: Arc::new(NodeSubprocessLoader),
            external_rules: None,
            external_reporters: None,
            cwd: None,
            stdout_is_tty: false,
        }
    }

    /// Replace how JS/TS configs are evaluated (the napi build calls back into the host Node).
    #[must_use]
    pub fn with_js_config_loader(mut self, loader: Box<dyn JsConfigLoader>) -> Self {
        self.js_loader = Arc::from(loader);
        self
    }

    /// Plug in JS plugin rules and reporters.
    #[must_use]
    pub fn with_plugins(
        mut self,
        rules: Arc<dyn ExternalRules>,
        reporters: Arc<dyn ExternalReporters>,
    ) -> Self {
        self.external_rules = Some(rules);
        self.external_reporters = Some(reporters);
        self
    }

    #[must_use]
    pub fn with_cwd(mut self, cwd: PathBuf) -> Self {
        self.cwd = Some(cwd);
        self
    }

    pub fn run_stdio(mut self) -> CliRunResult {
        self.stdout_is_tty = std::io::stdout().is_terminal();
        // No locks held across the run: `--lsp` writes the protocol to stdout from another thread.
        let mut out = std::io::BufWriter::new(std::io::stdout());
        let result = self.run(&mut out, &mut std::io::stderr());
        let _ = out.flush();
        result
    }

    fn parse(&self, out: &mut dyn Write, err: &mut dyn Write) -> Result<Cli, CliRunResult> {
        let argv = std::iter::once("oxcruise".to_string()).chain(self.args.iter().cloned());
        Cli::try_parse_from(argv).map_err(|e| {
            let text = e.to_string();
            if e.use_stderr() {
                let _ = write!(err, "{text}");
                CliRunResult::InvalidOptions
            } else {
                let _ = write!(out, "{text}");
                CliRunResult::Success
            }
        })
    }

    pub fn run(self, out: &mut dyn Write, err: &mut dyn Write) -> CliRunResult {
        let cli = match self.parse(out, err) {
            Ok(c) => c,
            Err(result) => return result,
        };
        match cli.mode() {
            Mode::Init => {
                let cwd = self.resolve_cwd(cli.cwd.as_deref());
                init::run(cli.config.as_deref(), &cwd, out, err)
            }
            Mode::Migrate => {
                let cwd = self.resolve_cwd(cli.cwd.as_deref());
                let source = cli.migrate.as_ref().and_then(|s| s.as_deref());
                migrate::run(
                    source,
                    cli.config.as_deref(),
                    &cwd,
                    self.js_loader.as_ref(),
                    out,
                    err,
                )
            }
            Mode::Rules => match cli.format.as_str() {
                "json" => {
                    rules_list::json(out);
                    CliRunResult::Success
                }
                "default" => {
                    rules_list::table(out);
                    CliRunResult::Success
                }
                other => fail(
                    err,
                    format!("`--rules` supports `-f json`, not `-f {other}`"),
                ),
            },
            Mode::PrintConfig => self.print_config(&cli, out, err),
            Mode::Lsp => {
                let (conn, io) = Connection::stdio();
                let result = self.serve_lsp(&cli, conn, err);
                let _ = io.join();
                result
            }
            Mode::Check => self.check(&cli, out, err),
        }
    }

    /// Run a check and return what it found instead of rendering a report (the `cruise()` API).
    /// Messages go to `err`; `Err` carries the failure that stopped the run.
    pub fn analyze_only(self, err: &mut dyn Write) -> Result<Analysis, CliRunResult> {
        let cli = self.parse(&mut std::io::sink(), err)?;
        let not_a_cruise = match cli.mode() {
            Mode::Check => return self.analyze(&cli, err),
            Mode::Init => "--init",
            Mode::Migrate => "--migrate",
            Mode::Rules => "--rules",
            Mode::PrintConfig => "--print-config",
            Mode::Lsp => "--lsp",
        };
        Err(fail(err, format!("`{not_a_cruise}` is not a cruise")))
    }

    /// Run the language server over `conn`, analysing each workspace root with this runner's
    /// config loader and plugins. Needs `--lsp` among the arguments.
    pub fn serve_lsp_on(self, conn: Connection) -> CliRunResult {
        let err = &mut std::io::stderr();
        let cli = match self.parse(&mut std::io::sink(), err) {
            Ok(cli) if cli.mode() == Mode::Lsp => cli,
            Ok(_) => return fail(err, "`serve_lsp_on` needs `--lsp`"),
            Err(result) => return result,
        };
        self.serve_lsp(&cli, conn, err)
    }

    fn serve_lsp(self, cli: &Cli, conn: Connection, err: &mut dyn Write) -> CliRunResult {
        // flags every analysis gets on top of `--cwd <root>`
        let mut base_args = vec![];
        if let Some(config) = &cli.config {
            base_args.extend(["--config".to_string(), config.display().to_string()]);
        }
        if let Some(threads) = cli.threads {
            base_args.extend(["--threads".to_string(), threads.to_string()]);
        }
        let analyze = Box::new(move |root: &Path| {
            let mut args = vec!["--cwd".to_string(), root.display().to_string()];
            args.extend(base_args.iter().cloned());
            let runner = CliRunner {
                args,
                js_loader: self.js_loader.clone(),
                external_rules: self.external_rules.clone(),
                external_reporters: self.external_reporters.clone(),
                cwd: None,
                stdout_is_tty: false,
            };
            let mut log = Vec::new();
            let analysis = runner
                .analyze_only(&mut log)
                .map(|a| LspAnalysis {
                    violations: violation_outs(&a.graph, &a.violations),
                    graph: a.graph,
                    cwd: a.cwd,
                })
                .map_err(|_| failure_message(&log));
            AnalyzeOutcome {
                analysis,
                log: String::from_utf8_lossy(&log).into_owned(),
            }
        });
        match oxcruise_language_server::serve(conn, analyze) {
            Ok(()) => CliRunResult::Success,
            Err(e) => fail(err, format!("language server: {e}")),
        }
    }

    fn resolve_cwd(&self, flag: Option<&Path>) -> PathBuf {
        let base = self
            .cwd
            .clone()
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        match flag {
            Some(p) if p.is_absolute() => p.to_path_buf(),
            Some(p) => base.join(p),
            None => base,
        }
    }

    /// On for a terminal unless `NO_COLOR` is set; a non-empty `FORCE_COLOR` (not `0`) forces it on.
    fn color(&self) -> bool {
        let forced = std::env::var_os("FORCE_COLOR").is_some_and(|v| !v.is_empty() && v != "0");
        forced || (self.stdout_is_tty && std::env::var_os("NO_COLOR").is_none())
    }

    fn print_config(&self, args: &Cli, out: &mut dyn Write, err: &mut dyn Write) -> CliRunResult {
        let cwd = match self.canonical_cwd(args) {
            Ok(c) => c,
            Err(e) => return fail(err, e),
        };
        let loaded = match config_io::load(&cwd, args.config.as_deref(), self.js_loader.as_ref()) {
            Ok(l) => l,
            Err(e) => return fail(err, e),
        };
        for w in &loaded.warnings {
            let _ = writeln!(err, "warning: {w}");
        }
        let mut config = loaded.config;
        for c in Category::ALL {
            config
                .categories
                .entry(c)
                .or_insert_with(|| c.default_level());
        }
        let json = serde_json::to_string_pretty(&config).expect("config serialises");
        let _ = writeln!(out, "{json}");
        CliRunResult::Success
    }

    fn canonical_cwd(&self, args: &Cli) -> Result<PathBuf, String> {
        match self.resolve_cwd(args.cwd.as_deref()).canonicalize() {
            Ok(c) => Ok(oxcruise_resolve::simplify_path(&c)),
            Err(e) => Err(format!("cannot use working directory: {e}")),
        }
    }

    fn check(&self, args: &Cli, out: &mut dyn Write, err: &mut dyn Write) -> CliRunResult {
        let format: OutputFormat = match args.format.parse() {
            Ok(f) => f,
            Err(e) => return fail(err, e),
        };
        let started = Instant::now();
        let analysis = match self.analyze(args, err) {
            Ok(a) => a,
            Err(result) => return result,
        };
        let Analysis {
            cwd,
            graph,
            workspace,
            violations,
            suppressed,
            errors,
            warnings,
        } = analysis;
        // the counts above already include the warnings that are not shown
        let violations = if args.quiet {
            violations
                .into_iter()
                .filter(|v| v.severity == Severity::Error)
                .collect()
        } else {
            violations
        };
        let result = CruiseResult::build(
            &graph,
            &workspace,
            cwd.clone(),
            &violations,
            suppressed,
            vec![],
        );

        let reporter_opts = ReporterOptions {
            color: self.color(),
            rule_docs: builtin_rules()
                .into_iter()
                .map(|r| (r.name.to_string(), r.docs.to_string()))
                .collect(),
        };
        if let Err(e) = self.write_report(&format, &result, &reporter_opts, out) {
            return fail(err, e);
        }
        if format == OutputFormat::Human || format == OutputFormat::Long {
            let _ = writeln!(
                err,
                "finished in {:.0?} on {} thread(s)",
                started.elapsed(),
                threads(args.threads)
            );
        }

        if errors > 0 {
            return CliRunResult::FoundErrors { errors };
        }
        if args.deny_warnings && warnings > 0 {
            let _ = writeln!(
                err,
                "error: found {warnings} warning(s) and --deny-warnings is set"
            );
            return CliRunResult::NoWarningsAllowed { warnings };
        }
        if let Some(max) = args.max_warnings
            && warnings > max as usize
        {
            let _ = writeln!(
                err,
                "error: found {warnings} warning(s), which exceeds the maximum of {max}"
            );
            return CliRunResult::MaxWarningsExceeded { warnings, max };
        }
        CliRunResult::Success
    }

    /// Everything up to the report: config, graph, rules.
    fn analyze(&self, args: &Cli, err: &mut dyn Write) -> Result<Analysis, CliRunResult> {
        let cwd = match self.canonical_cwd(args) {
            Ok(c) => c,
            Err(e) => return Err(fail(err, e)),
        };

        let loaded = match config_io::load(&cwd, args.config.as_deref(), self.js_loader.as_ref()) {
            Ok(l) => l,
            Err(e) => return Err(fail(err, e)),
        };
        for w in &loaded.warnings {
            let _ = writeln!(err, "warning: {w}");
        }
        let config = loaded.config;

        if !config.js_plugins.is_empty() && self.external_rules.is_none() {
            return Err(fail(
                err,
                "`jsPlugins` need the npm package: run oxcruise through `npx oxcruise` (the standalone binary has no JS runtime)",
            ));
        }

        let plugins: Vec<PluginRef> = config
            .js_plugins
            .iter()
            .map(|p| PluginRef {
                specifier: p.specifier().to_string(),
                alias: p.alias().map(str::to_string),
            })
            .collect();
        if let Some(rules) = &self.external_rules
            && let Err(e) = rules.load_plugins(&plugins, &cwd)
        {
            return Err(fail(err, e));
        }
        let engine = match Engine::new(&config, self.external_rules.clone()) {
            Ok(e) => e,
            Err(errors) => {
                for e in errors {
                    let _ = writeln!(err, "error: {e}");
                }
                return Err(CliRunResult::InvalidOptions);
            }
        };
        let opts = match options::build(&cwd, &config, args) {
            Ok(o) => o,
            Err(e) => return Err(fail(err, e)),
        };
        let output = match cruise(&opts) {
            Ok(o) => o,
            Err(e) => return Err(fail(err, e)),
        };
        let run = engine.run(&output.graph, &cwd, &output.workspace);
        if !run.fatal.is_empty() {
            for f in &run.fatal {
                let _ = writeln!(err, "error: {f}");
            }
            return Err(CliRunResult::InvalidOptions);
        }
        let (errors, warnings) = (
            run.count(oxcruise_core::Severity::Error),
            run.count(oxcruise_core::Severity::Warn),
        );
        // problems with the config itself (a forbid rule that matches nothing) go to stderr
        for w in &run.warnings {
            let _ = writeln!(err, "warning: {w}");
        }
        Ok(Analysis {
            cwd,
            graph: output.graph,
            workspace: output.workspace,
            violations: run.violations,
            suppressed: run.suppressed,
            errors,
            warnings,
        })
    }

    fn write_report(
        &self,
        format: &OutputFormat,
        result: &CruiseResult,
        opts: &ReporterOptions,
        out: &mut dyn Write,
    ) -> Result<(), String> {
        let ext = self.external_reporters.as_deref();
        render(format, result, opts, ext, out).map_err(|e| e.to_string())
    }
}

/// The last `error:` line a failed run wrote (or all of it), without the prefix.
fn failure_message(log: &[u8]) -> String {
    let text = String::from_utf8_lossy(log);
    let errors: Vec<&str> = text
        .lines()
        .filter_map(|l| l.strip_prefix("error: "))
        .collect();
    if errors.is_empty() {
        text.trim().to_string()
    } else {
        errors.join("\n")
    }
}

fn threads(n: Option<usize>) -> usize {
    n.unwrap_or_else(|| std::thread::available_parallelism().map_or(1, usize::from))
}

fn fail(err: &mut dyn Write, msg: impl std::fmt::Display) -> CliRunResult {
    let _ = writeln!(err, "error: {msg}");
    CliRunResult::InvalidOptions
}
