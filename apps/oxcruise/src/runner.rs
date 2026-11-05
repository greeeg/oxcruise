use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use clap::Parser;
use oxcruise_config::{Category, JsConfigLoader, NodeSubprocessLoader};
use oxcruise_core::ViolationLevel;
use oxcruise_graph::analyses::ExportUsage;
use oxcruise_graph::cruise;
use oxcruise_report::{
    CruiseResult, ExternalReporters, OutputFormat, ReporterOptions, Totals, render,
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
    /// Which exports are used, when symbols were extracted.
    pub usage: Option<ExportUsage>,
    pub errors: usize,
    pub warnings: usize,
}

pub struct CliRunner {
    args: Vec<String>,
    js_loader: Box<dyn JsConfigLoader>,
    external_rules: Option<Arc<dyn ExternalRules>>,
    external_reporters: Option<Arc<dyn ExternalReporters>>,
    cwd: Option<PathBuf>,
    stdout_is_tty: bool,
}

impl CliRunner {
    pub fn new(args: Vec<String>) -> Self {
        Self {
            args,
            js_loader: Box::new(NodeSubprocessLoader),
            external_rules: None,
            external_reporters: None,
            cwd: None,
            stdout_is_tty: false,
        }
    }

    /// Replace how JS/TS configs are evaluated (the napi build calls back into the host Node).
    #[must_use]
    pub fn with_js_config_loader(mut self, loader: Box<dyn JsConfigLoader>) -> Self {
        self.js_loader = loader;
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
        let (stdout, stderr) = (std::io::stdout(), std::io::stderr());
        let mut out = std::io::BufWriter::new(stdout.lock());
        let result = self.run(&mut out, &mut stderr.lock());
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
            Mode::Visualize => self.visualize(&cli, err),
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
            Mode::Visualize => "--visualize",
        };
        Err(fail(err, format!("`{not_a_cruise}` is not a cruise")))
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
            usage,
            errors,
            warnings,
        } = analysis;
        let (violations, errors, warnings) = if args.fix {
            match crate::fix::apply(&cwd, violations) {
                Ok(applied) => {
                    let count = |s| applied.fixed.iter().filter(|v| v.level == s).count();
                    let (fixed_errors, fixed_warnings) =
                        (count(ViolationLevel::Error), count(ViolationLevel::Warn));
                    if !applied.fixed.is_empty() {
                        let _ = writeln!(
                            err,
                            "fixed {} problem(s); run again to see what that unlocked",
                            applied.fixed.len()
                        );
                    }
                    (
                        applied.remaining,
                        errors - fixed_errors,
                        warnings - fixed_warnings,
                    )
                }
                Err(e) => return fail(err, e),
            }
        } else {
            (violations, errors, warnings)
        };
        // the counts above already include the warnings that are not shown
        let violations = if args.quiet {
            violations
                .into_iter()
                .filter(|v| v.level == ViolationLevel::Error)
                .collect()
        } else {
            violations
        };
        let result = result_of(
            &cwd,
            &graph,
            &workspace,
            &violations,
            // `--quiet` hides warnings, it does not make them go away
            Totals {
                error: errors,
                warn: warnings,
                suppressed,
            },
            usage.as_ref(),
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

    /// Write the architecture site. It shows the graph, so it does not gate: violations do not fail it.
    fn visualize(&self, args: &Cli, err: &mut dyn Write) -> CliRunResult {
        let analysis = match self.analyze(args, err) {
            Ok(a) => a,
            Err(result) => return result,
        };
        let result = result_of(
            &analysis.cwd,
            &analysis.graph,
            &analysis.workspace,
            &analysis.violations,
            Totals {
                error: analysis.errors,
                warn: analysis.warnings,
                suppressed: analysis.suppressed,
            },
            analysis.usage.as_ref(),
        );
        let dir = match args.visualize.as_ref().and_then(|d| d.as_deref()) {
            Some(d) if d.is_absolute() => d.to_path_buf(),
            Some(d) => analysis.cwd.join(d),
            None => analysis.cwd.join("oxcruise-report"),
        };
        if let Err(e) = oxcruise_visualize::write_site(&dir, &result) {
            return fail(err, format!("cannot write {}: {e}", dir.display()));
        }
        let _ = writeln!(
            err,
            "wrote {} ({} modules, {} violations)",
            dir.join("index.html").display(),
            result.modules.len(),
            result.violations.len()
        );
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
            Ok(mut e) => {
                if args.production {
                    e.set_production();
                }
                e
            }
            Err(errors) => {
                for e in errors {
                    let _ = writeln!(err, "error: {e}");
                }
                return Err(CliRunResult::InvalidOptions);
            }
        };
        let mut opts = match options::build(&cwd, &config, args) {
            Ok(o) => o,
            Err(e) => return Err(fail(err, e)),
        };
        // symbols cost a second walk over every file, so they are extracted only when asked for
        // (`symbols`, `--symbols`), or when a rule or a plugin reads them
        opts.extract.symbols = engine.wants_symbols()
            || config.symbols
            || args.symbols
            || self
                .external_rules
                .as_ref()
                .is_some_and(|x| x.needs_symbols());
        // what is used depends on the manifests (entry points), whichever rules are on
        opts.manifests = engine.wants_manifests() || opts.extract.symbols;
        opts.manifest_peers = engine.wants_peers();
        let symbols = opts.extract.symbols;
        let output = match cruise(&opts) {
            Ok(o) => o,
            Err(e) => return Err(fail(err, e)),
        };
        let run = engine.run(
            &output.graph,
            &cwd,
            &output.workspace,
            &output.manifests,
            symbols,
        );
        if !run.fatal.is_empty() {
            for f in &run.fatal {
                let _ = writeln!(err, "error: {f}");
            }
            return Err(CliRunResult::InvalidOptions);
        }
        let (errors, warnings) = (
            run.count(oxcruise_core::ViolationLevel::Error),
            run.count(oxcruise_core::ViolationLevel::Warn),
        );
        // problems with the config itself (a forbid rule that matches nothing) go to stderr
        for w in &run.warnings {
            let _ = writeln!(err, "warning: {w}");
        }
        let usage = run
            .usage
            .or_else(|| symbols.then(|| ExportUsage::empty(output.graph.module_count())));
        Ok(Analysis {
            cwd,
            graph: output.graph,
            workspace: output.workspace,
            violations: run.violations,
            suppressed: run.suppressed,
            usage,
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

/// The result of an analysis, as the reporters and the site read it.
fn result_of(
    cwd: &Path,
    graph: &oxcruise_core::Graph,
    workspace: &[oxcruise_resolve::WorkspacePackage],
    violations: &[oxcruise_core::Violation],
    totals: Totals,
    usage: Option<&ExportUsage>,
) -> CruiseResult {
    let used = usage.map(|u| move |m, name: &str| u.is_used(m, name));
    CruiseResult::build(
        graph,
        workspace,
        cwd.to_path_buf(),
        violations,
        totals,
        vec![],
        used.as_ref().map(|f| f as &oxcruise_report::ExportUsed<'_>),
    )
}

fn threads(n: Option<usize>) -> usize {
    n.unwrap_or_else(|| std::thread::available_parallelism().map_or(1, usize::from))
}

fn fail(err: &mut dyn Write, msg: impl std::fmt::Display) -> CliRunResult {
    let _ = writeln!(err, "error: {msg}");
    CliRunResult::InvalidOptions
}
