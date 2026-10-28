use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use clap::Parser;
use oxcruise_config::{JsConfigLoader, NodeSubprocessLoader};
use oxcruise_graph::cruise;
use oxcruise_report::{CruiseResult, ExternalReporters, OutputFormat, ReporterOptions, render};
use oxcruise_rules::{Engine, ExternalRules, PluginRef, builtin_rules};

use crate::args::{CheckArgs, Cli, ColorChoice, Command};
use crate::result::CliRunResult;
use crate::{config_io, migrate, options};

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
        match cli.command {
            None => self.check(&cli.check, out, err),
            Some(Command::Check(a)) => self.check(&a, out, err),
            Some(Command::Migrate(a)) => {
                let cwd = self.resolve_cwd(a.cwd.as_deref());
                migrate::run(&a, &cwd, self.js_loader.as_ref(), out, err)
            }
        }
    }

    /// Run a check and return what it found instead of rendering a report (the `cruise()` API).
    /// Messages go to `err`; `Err` carries the failure that stopped the run.
    pub fn analyze_only(self, err: &mut dyn Write) -> Result<Analysis, CliRunResult> {
        let cli = self.parse(&mut std::io::sink(), err)?;
        match &cli.command {
            None => self.analyze(&cli.check, err),
            Some(Command::Check(a)) => self.analyze(a, err),
            Some(Command::Migrate(_)) => Err(fail(err, "`migrate` is not a cruise")),
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

    fn color(&self, choice: ColorChoice, to_file: bool) -> bool {
        match choice {
            ColorChoice::Always => true,
            ColorChoice::Never => false,
            ColorChoice::Auto => {
                self.stdout_is_tty && !to_file && std::env::var_os("NO_COLOR").is_none()
            }
        }
    }

    fn check(&self, args: &CheckArgs, out: &mut dyn Write, err: &mut dyn Write) -> CliRunResult {
        if args.rules {
            let list: Vec<_> = builtin_rules()
                .into_iter()
                .map(|r| {
                    serde_json::json!({
                        "name": r.name, "category": r.category.as_str(), "version": r.version,
                        "docs": r.docs,
                    })
                })
                .collect();
            let _ = writeln!(
                out,
                "{}",
                serde_json::to_string_pretty(&list).expect("json")
            );
            return CliRunResult::Success;
        }

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
        let result = CruiseResult::build(
            &graph,
            &workspace,
            cwd.clone(),
            &violations,
            suppressed,
            vec![],
        );

        let reporter_opts = ReporterOptions {
            color: self.color(args.color, args.output.is_some()),
            rule_docs: builtin_rules()
                .into_iter()
                .map(|r| (r.name.to_string(), r.docs.to_string()))
                .collect(),
        };
        if let Err(e) = self.write_report(
            &format,
            &result,
            &reporter_opts,
            args.output.as_deref(),
            &cwd,
            out,
        ) {
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
    fn analyze(&self, args: &CheckArgs, err: &mut dyn Write) -> Result<Analysis, CliRunResult> {
        let cwd = match self.resolve_cwd(args.cwd.as_deref()).canonicalize() {
            Ok(c) => oxcruise_resolve::simplify_path(&c),
            Err(e) => return Err(fail(err, format!("cannot use working directory: {e}"))),
        };

        let loaded = match config_io::load(
            &cwd,
            args.config.as_deref(),
            args.no_config,
            self.js_loader.as_ref(),
        ) {
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
        output: Option<&Path>,
        cwd: &Path,
        stdout: &mut dyn Write,
    ) -> Result<(), String> {
        let ext = self.external_reporters.as_deref();
        match output {
            Some(p) => {
                let p = if p.is_absolute() {
                    p.to_path_buf()
                } else {
                    cwd.join(p)
                };
                let mut file = std::io::BufWriter::new(
                    std::fs::File::create(&p)
                        .map_err(|e| format!("cannot create {}: {e}", p.display()))?,
                );
                render(format, result, opts, ext, &mut file).map_err(|e| e.to_string())?;
                file.flush().map_err(|e| e.to_string())
            }
            None => render(format, result, opts, ext, stdout).map_err(|e| e.to_string()),
        }
    }
}

fn threads(n: Option<usize>) -> usize {
    n.unwrap_or_else(|| std::thread::available_parallelism().map_or(1, usize::from))
}

fn fail(err: &mut dyn Write, msg: impl std::fmt::Display) -> CliRunResult {
    let _ = writeln!(err, "error: {msg}");
    CliRunResult::InvalidOptions
}
