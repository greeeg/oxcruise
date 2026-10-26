use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use clap::Parser;
use oxcruise_config::{JsConfigLoader, NodeSubprocessLoader};
use oxcruise_graph::cruise;
use oxcruise_report::{CruiseResult, ExternalReporters, OutputFormat, ReporterOptions, render};
use oxcruise_rules::{Engine, ExternalRules, PluginRef, builtin_rules};

use crate::args::{CheckArgs, Cli, ColorChoice, Command, FmtArgs};
use crate::result::CliRunResult;
use crate::{config_io, migrate, options};

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

    pub fn run(self, out: &mut dyn Write, err: &mut dyn Write) -> CliRunResult {
        let argv = std::iter::once("oxcruise".to_string()).chain(self.args.iter().cloned());
        let cli = match Cli::try_parse_from(argv) {
            Ok(c) => c,
            Err(e) => {
                let text = e.to_string();
                return if e.use_stderr() {
                    let _ = write!(err, "{text}");
                    CliRunResult::InvalidOptions
                } else {
                    let _ = write!(out, "{text}");
                    CliRunResult::Success
                };
            }
        };
        match cli.command {
            None => self.check(&cli.check, out, err),
            Some(Command::Check(a)) => self.check(&a, out, err),
            Some(Command::Fmt(a)) => self.fmt(&a, out, err),
            Some(Command::Migrate(a)) => {
                let cwd = self.resolve_cwd(a.cwd.as_deref());
                migrate::run(&a, &cwd, self.js_loader.as_ref(), out, err)
            }
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
                        "name": r.name, "category": r.category, "version": r.version,
                        "recommended": r.recommended, "docs": r.docs,
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
        let cwd = match self.resolve_cwd(args.cwd.as_deref()).canonicalize() {
            Ok(c) => c,
            Err(e) => return fail(err, format!("cannot use working directory: {e}")),
        };

        let loaded = match config_io::load(
            &cwd,
            args.config.as_deref(),
            args.no_config,
            self.js_loader.as_ref(),
        ) {
            Ok(l) => l,
            Err(e) => return fail(err, e),
        };
        for w in &loaded.warnings {
            let _ = writeln!(err, "warning: {w}");
        }
        if loaded.imported_from_depcruise {
            let _ = writeln!(
                err,
                "note: using {} through the dependency-cruiser importer; run `oxcruise migrate` to convert it",
                loaded
                    .path
                    .as_deref()
                    .map_or_else(String::new, |p| p.display().to_string())
            );
        }
        let config = loaded.config;

        if !config.js_plugins.is_empty() && self.external_rules.is_none() {
            return fail(
                err,
                "`jsPlugins` need the npm package: run oxcruise through `npx oxcruise` (the standalone binary has no JS runtime)",
            );
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
            return fail(err, e);
        }
        let engine = match Engine::new(&config, self.external_rules.clone()) {
            Ok(e) => e,
            Err(errors) => {
                for e in errors {
                    let _ = writeln!(err, "error: {e}");
                }
                return CliRunResult::InvalidOptions;
            }
        };
        let opts = match options::build(&cwd, &config, args) {
            Ok(o) => o,
            Err(e) => return fail(err, e),
        };
        let output = match cruise(&opts) {
            Ok(o) => o,
            Err(e) => return fail(err, e),
        };
        let run = engine.run(&output.graph, &cwd, &output.workspace);
        if !run.fatal.is_empty() {
            for f in &run.fatal {
                let _ = writeln!(err, "error: {f}");
            }
            return CliRunResult::InvalidOptions;
        }
        let (errors, warnings) = (
            run.count(oxcruise_core::Severity::Error),
            run.count(oxcruise_core::Severity::Warn),
        );
        let result = CruiseResult::build(
            &output.graph,
            &output.workspace,
            cwd.clone(),
            &run.violations,
            run.suppressed,
            run.warnings,
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
        if format == OutputFormat::Human || format == OutputFormat::ErrLong {
            let _ = writeln!(
                err,
                "finished in {:.0?} on {} thread(s)",
                started.elapsed(),
                threads(args.threads)
            );
        }

        if errors > 0 {
            return CliRunResult::FoundErrors {
                errors,
                compat: args.compat,
            };
        }
        if args.deny_warnings && warnings > 0 {
            let _ = writeln!(
                err,
                "error: found {warnings} warning(s) and --deny-warnings is set"
            );
            return CliRunResult::NoWarningsAllowed { warnings };
        }
        if let Some(max) = args.max_warnings.or(config.options.max_warnings)
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

    fn fmt(&self, args: &FmtArgs, out: &mut dyn Write, err: &mut dyn Write) -> CliRunResult {
        let format: OutputFormat = match args.format.parse() {
            Ok(f) => f,
            Err(e) => return fail(err, e),
        };
        let cwd = self.resolve_cwd(None);
        let path = if args.input.is_absolute() {
            args.input.clone()
        } else {
            cwd.join(&args.input)
        };
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => return fail(err, format!("cannot read {}: {e}", path.display())),
        };
        let mut result: CruiseResult = match serde_json::from_str(&text) {
            Ok(r) => r,
            Err(e) => {
                return fail(
                    err,
                    format!("{} is not an oxcruise `json` result: {e}", path.display()),
                );
            }
        };
        result.root = cwd.clone();
        let opts = ReporterOptions {
            color: self.color(args.color, args.output.is_some()),
            rule_docs: builtin_rules()
                .into_iter()
                .map(|r| (r.name.to_string(), r.docs.to_string()))
                .collect(),
        };
        match self.write_report(&format, &result, &opts, args.output.as_deref(), &cwd, out) {
            Ok(()) => CliRunResult::Success,
            Err(e) => fail(err, e),
        }
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
