mod body;
mod usage;

pub use usage::CHECK_USAGE;

use std::fs;

use body::diag::Diag;
use body::syntax;

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
    Exit {
        code: 0,
        stdout: String::new(),
        stderr: String::new(),
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
pub fn test(path: &str, _opts: Opts) -> Exit {
    let _test_channel = crate::body::diag::status;
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
    Exit {
        code: 0,
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
    }
}
