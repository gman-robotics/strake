mod body;

/// Usage line for `strake check` (includes `[--blob-root DIR]` for T8).
pub const CHECK_USAGE: &str =
    "usage: strake check [--draft] [--fn NAME] [--blob-root DIR] <file|dir>";

use std::fs;

use body::diag::{self, Diag, ExportOutcome};
use body::host::EmptyHost;
use body::syntax;
use body::{compile_module, lower_abort_message, CompileError, LOWER_ABORT_EXIT};

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

/// Message printed for `strake check` usage errors (must match [`CHECK_USAGE`]).
pub fn check_usage_stderr() -> String {
    format!("{CHECK_USAGE}\n")
}

fn read_source(path: &str) -> Result<String, Exit> {
    fs::read_to_string(path).map_err(|e| Exit {
        code: 2,
        stdout: String::new(),
        stderr: format!("{e}\n"),
    })
}

fn diag_stderr(diag: &Diag) -> String {
    let mut buf = Vec::new();
    diag.write_json_line(&mut buf).expect("json diag line");
    String::from_utf8(buf).expect("utf8 diag")
}

fn parse_fail(diag: Diag) -> Exit {
    Exit {
        code: 1,
        stdout: String::new(),
        stderr: diag_stderr(&diag),
    }
}

fn check_fail(diags: Vec<Diag>) -> Exit {
    let stderr = diags.iter().map(diag_stderr).collect::<String>();
    Exit {
        code: 1,
        stdout: String::new(),
        stderr,
    }
}

fn codeless_exit(msg: &str) -> Exit {
    Exit {
        code: 3,
        stdout: String::new(),
        stderr: format!("{msg}\n"),
    }
}

/// `strake check [--draft] [--fn NAME] [--blob-root DIR] <file|dir>`
pub fn check(path: &str, _opts: Opts) -> Exit {
    let source = match read_source(path) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let module = match syntax::parse(&source, path) {
        Ok(m) => m,
        Err(diag) => return parse_fail(diag),
    };
    if let Some(msg) = syntax::codeless_rejection(&module) {
        return codeless_exit(msg);
    }
    match compile_module(&module, _opts.draft) {
        Ok(_) => Exit {
            code: 0,
            stdout: String::new(),
            stderr: String::new(),
        },
        Err(CompileError::Check(diags)) => check_fail(diags),
        Err(CompileError::Lower(err)) => Exit {
            code: LOWER_ABORT_EXIT,
            stdout: String::new(),
            stderr: lower_abort_message(&err),
        },
    }
}

/// `strake fmt <file>`
pub fn fmt(path: &str) -> Exit {
    let source = match read_source(path) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let module = match syntax::parse(&source, path) {
        Ok(m) => m,
        Err(diag) => return parse_fail(diag),
    };
    Exit {
        code: 0,
        stdout: syntax::format(&module),
        stderr: String::new(),
    }
}

/// `strake test [--draft] <file>`
pub fn test(path: &str, opts: Opts) -> Exit {
    let source = match read_source(path) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let module = match syntax::parse(&source, path) {
        Ok(m) => m,
        Err(diag) => return parse_fail(diag),
    };
    if let Some(msg) = syntax::codeless_rejection(&module) {
        return codeless_exit(msg);
    }
    let program = match compile_module(&module, opts.draft) {
        Ok(p) => p,
        Err(CompileError::Check(diags)) => return check_fail(diags),
        Err(CompileError::Lower(err)) => {
            return Exit {
                code: LOWER_ABORT_EXIT,
                stdout: String::new(),
                stderr: lower_abort_message(&err),
            };
        }
    };
    let host = EmptyHost;
    let outcomes: Vec<ExportOutcome> = body::interp::run_test_exports(&program, &host);
    let code = diag::status(&outcomes);
    Exit {
        code,
        stdout: String::new(),
        stderr: String::new(),
    }
}

/// `strake run --fn NAME [--draft] <file>`
pub fn run(_path: &str, _fn_name: &str, _opts: Opts) -> Exit {
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
    fn check_usage_includes_blob_root() {
        assert!(CHECK_USAGE.contains("[--blob-root DIR]"));
        assert_eq!(check_usage_stderr(), format!("{CHECK_USAGE}\n"));
    }

    #[test]
    fn check_eparse_exits_1_with_catalog_code() {
        let tmp = std::env::temp_dir().join("strake_eparse_cli.strake");
        std::fs::write(&tmp, "fn f() -> unit\n  ret unit\nend\n").expect("write");
        let exit = check(tmp.to_str().unwrap(), Opts::default());
        assert_eq!(exit.code, 1);
        assert!(exit.stderr.contains("\"code\""));
        assert!(exit.stderr.contains("E_PARSE"));
    }

    #[test]
    fn check_duplicate_fuel_codeless_not_catalog() {
        let src = "strake 1\nfuel 1\nfuel 2\nfn f() -> unit\n  ret unit\nend\n";
        let tmp = std::env::temp_dir().join("strake_fuel_twice_cli.strake");
        std::fs::write(&tmp, src).expect("write");
        let exit = check(tmp.to_str().unwrap(), Opts::default());
        assert_eq!(exit.code, 3);
        assert!(!exit.stderr.contains("\"code\""));
        assert!(!exit.stderr.contains("E_LOWER"));
    }

    #[test]
    fn lower_abort_exit_is_plain_70() {
        let src = "strake 1\n\nfn f() -> i64\n  x = sub i64 1 2\n  ret x\nend\n\ntest t\n  ret 0\nend\n";
        let tmp = std::env::temp_dir().join("strake_lib_lower70.strake");
        std::fs::write(&tmp, src).expect("write");
        let exit = test(tmp.to_str().unwrap(), Opts::default());
        assert_eq!(exit.code, body::LOWER_ABORT_EXIT);
        assert!(!exit.stderr.contains("\"code\""));
        assert!(exit.stderr.contains("lower:"));
    }

    #[test]
    fn fmt_eparse_exits_1() {
        let tmp = std::env::temp_dir().join("strake_fmt_eparse.strake");
        std::fs::write(&tmp, "fn f() -> unit\n  ret unit\nend\n").expect("write");
        let exit = fmt(tmp.to_str().unwrap());
        assert_eq!(exit.code, 1);
        assert!(exit.stderr.contains("E_PARSE"));
    }
}
