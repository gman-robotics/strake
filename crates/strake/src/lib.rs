mod body;

pub use body::diag::{status, Code, Diag, ExportOutcome, Kind, Span};

/// CLI options shared across commands.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Opts {
    pub draft: bool,
    pub only_fn: Option<String>,
    pub blob_root: Option<String>,
}

/// Process exit bundle for the five public entrypoints.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Exit {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

fn usage(message: &str) -> Exit {
    Exit {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n"),
    }
}

/// `strake check [--draft] [--fn NAME] [--blob-root DIR] <file|dir>`
pub fn check(_path: &str, _opts: Opts) -> Exit {
    usage("usage: strake check [--draft] [--fn NAME] [--blob-root DIR] <file|dir>")
}

/// `strake fmt <file>`
pub fn fmt(_path: &str) -> Exit {
    usage("usage: strake fmt <file>")
}

/// `strake test [--draft] <file>`
pub fn test(_path: &str, _opts: Opts) -> Exit {
    usage("usage: strake test [--draft] <file>")
}

/// `strake run --fn NAME [--draft] <file>`
pub fn run(_path: &str, _fn_name: &str, _opts: Opts) -> Exit {
    // `status` is for the test channel only; `run` never calls it.
    usage("usage: strake run --fn NAME [--draft] <file>")
}

/// `strake build` — post-MVP stub (SPEC "MVP").
pub fn build() -> Exit {
    Exit {
        code: 2,
        stdout: "build is post-MVP\n".into(),
        stderr: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_exit_and_stderr() {
        let exit = build();
        assert_eq!(exit.code, 2);
        assert_eq!(exit.stdout, "build is post-MVP\n");
        assert!(!exit.stderr.contains("code"));
    }

    #[test]
    fn run_does_not_use_status() {
        let exit = run("x.strake", "main", Opts::default());
        assert_eq!(exit.code, 2);
        assert!(!exit.stderr.contains("code"));
    }
}
