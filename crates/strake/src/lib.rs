mod body;
mod run_cmd;

/// Usage line for `strake check` (includes `[--blob-root DIR]` for T8).
pub const CHECK_USAGE: &str =
    "usage: strake check [--draft] [--fn NAME] [--blob-root DIR] <file|dir>";

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
    usage(CHECK_USAGE)
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
pub fn run(path: &str, fn_name: &str, opts: Opts) -> Exit {
    run_cmd::run(path, fn_name, opts)
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
    fn check_usage_includes_blob_root() {
        assert!(
            CHECK_USAGE.contains("[--blob-root DIR]"),
            "check usage must document --blob-root for T8"
        );
        let exit = check("x.strake", Opts::default());
        assert_eq!(exit.stderr, format!("{CHECK_USAGE}\n"));
    }

    #[test]
    fn run_module_does_not_reference_status() {
        const RUN_SRC: &str = include_str!("run_cmd.rs");
        assert!(
            !RUN_SRC.contains("status(") && !RUN_SRC.contains("test_channel::"),
            "run must not call test-channel status"
        );
    }
}
