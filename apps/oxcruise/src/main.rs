use std::process::Termination;

use oxcruise::CliRunner;

fn main() -> impl Termination {
    CliRunner::new(std::env::args().skip(1).collect()).run_stdio()
}
