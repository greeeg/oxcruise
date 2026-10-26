//! Standalone binary. With the `napi` feature this crate is built as a Node addon instead
//! (see `run.rs`), and the binary is only a stub: it must not reference the library, because
//! napi symbols only exist inside a running Node process.

#[cfg(not(feature = "napi"))]
fn main() -> impl std::process::Termination {
    oxcruise::CliRunner::new(std::env::args().skip(1).collect()).run_stdio()
}

#[cfg(feature = "napi")]
fn main() {
    eprintln!(
        "this oxcruise was built as a Node addon (feature `napi`); install the npm package or build without that feature"
    );
    std::process::exit(2);
}
