//! `run` entrypoint — kept separate from the test-channel exit aggregator in `body::diag`.

use crate::{Exit, Opts};

fn usage(message: &str) -> Exit {
    Exit {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n"),
    }
}

/// `strake run --fn NAME [--draft] <file>`
pub fn run(_path: &str, _fn_name: &str, _opts: Opts) -> Exit {
    usage("usage: strake run --fn NAME [--draft] <file>")
}
