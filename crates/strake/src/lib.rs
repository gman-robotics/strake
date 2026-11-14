mod body;

/// Usage line for `strake check` (includes `[--blob-root DIR]` for T8).
pub const CHECK_USAGE: &str =
    "usage: strake check [--draft] [--fn NAME] [--blob-root DIR] <file|dir>";

use std::fs;
use std::path::{Path, PathBuf};

use body::check;
use body::diag::{self, Diag, ExportOutcome};
use body::host::EmptyHost;
use body::interp::Program;
use body::syntax;
use body::{compile_module_with_blobs, lower_abort_message, CompileError, LOWER_ABORT_EXIT};

#[cfg(test)]
thread_local! {
    static EXEC_ENTRIES_ON_THREAD: std::cell::Cell<usize> = std::cell::Cell::default();
}

#[cfg(test)]
/// Exec entries on this thread. Parallel `cargo test` workers do not share it.
pub(crate) fn exec_entries_on_this_thread() -> usize {
    EXEC_ENTRIES_ON_THREAD.with(std::cell::Cell::get)
}

#[cfg(test)]
pub(crate) fn note_exec_on_this_thread() {
    EXEC_ENTRIES_ON_THREAD.with(|entries| entries.set(entries.get() + 1));
}

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

fn collect_src_strake_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let src = dir.join("src");
    if !src.is_dir() {
        return out;
    }
    collect_strake_recursive(&src, &mut out);
    out.sort();
    out
}

fn collect_strake_recursive(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_strake_recursive(&path, out);
        } else if path.extension().is_some_and(|e| e == "strake") {
            out.push(path);
        }
    }
}

fn map_compile_error(err: CompileError) -> Exit {
    match err {
        CompileError::Check(check::CheckFailure::Diags(diags)) => check_fail(diags),
        CompileError::Check(check::CheckFailure::Codeless(msg)) => codeless_exit(msg),
        CompileError::Lower(err) => Exit {
            code: LOWER_ABORT_EXIT,
            stdout: String::new(),
            stderr: lower_abort_message(&err),
        },
    }
}

fn load_checked_module(path: &str) -> Result<syntax::Module, Exit> {
    let source = read_source(path)?;
    let module = syntax::parse(&source, path).map_err(parse_fail)?;
    if let Some(msg) = syntax::codeless_rejection(&module) {
        return Err(codeless_exit(msg));
    }
    Ok(module)
}

fn apply_only_fn_filter(module: &mut syntax::Module, name: &str) -> Result<(), Exit> {
    if !export_fn_exists(module, name) {
        return Err(Exit {
            code: 2,
            stdout: String::new(),
            stderr: format!("unknown export `{name}`\n"),
        });
    }
    filter_tests_for_only_fn(module, name);
    Ok(())
}

fn compile_path(path: &str, opts: &Opts, only_fn: Option<&str>) -> Result<Program, Exit> {
    let mut module = load_checked_module(path)?;
    if let Some(name) = only_fn {
        apply_only_fn_filter(&mut module, name)?;
    }
    let blob_root = opts.blob_root.as_deref().map(Path::new);
    compile_module_with_blobs(&module, opts.draft, blob_root).map_err(map_compile_error)
}

fn check_one_file(path: &str, opts: &Opts) -> Exit {
    match compile_path(path, opts, opts.only_fn.as_deref()) {
        Ok(_) => Exit {
            code: 0,
            stdout: String::new(),
            stderr: String::new(),
        },
        Err(exit) => exit,
    }
}

fn export_fn_exists(module: &syntax::Module, name: &str) -> bool {
    module.items.iter().any(|item| {
        matches!(
            item,
            syntax::Item::Fn(f) if f.name == name && !f.private
        )
    })
}

fn filter_tests_for_only_fn(module: &mut syntax::Module, only_fn: &str) {
    let module_ro = module.clone();
    module.items.retain(|item| match item {
        syntax::Item::Test(t) => check::test_matches_only_fn(t, only_fn, &module_ro),
        _ => true,
    });
}

/// `strake check [--draft] [--fn NAME] [--blob-root DIR] <file|dir>`
pub fn check(path: &str, opts: Opts) -> Exit {
    let p = Path::new(path);
    if p.is_dir() {
        let files = collect_src_strake_files(p);
        if files.is_empty() {
            return usage("no src/**/*.strake files found in directory");
        }
        let mut last = Exit {
            code: 0,
            stdout: String::new(),
            stderr: String::new(),
        };
        for f in files {
            let fp = f.to_str().unwrap();
            let exit = check_one_file(fp, &opts);
            if exit.code != 0 {
                return exit;
            }
            last = exit;
        }
        return last;
    }
    check_one_file(path, &opts)
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
/// Parse, codeless gate, check, and lower for `strake test`; `Err` is the exit.
fn compile_for_test(path: &str, opts: &Opts) -> Result<Program, Exit> {
    compile_path(path, opts, None)
}

pub fn test(path: &str, opts: Opts) -> Exit {
    let program = match compile_for_test(path, &opts) {
        Ok(p) => p,
        Err(exit) => return exit,
    };
    let host = EmptyHost;
    let runs = body::interp::run_test_exports(&program, &host);
    if let Some(msg) = runs.iter().find_map(|r| r.halt.as_deref()) {
        return codeless_exit(msg);
    }
    let outcomes: Vec<ExportOutcome> = runs.iter().map(|r| r.outcome).collect();
    let code = diag::status(&outcomes);
    let stderr = runs
        .iter()
        .flat_map(|r| r.diags.iter())
        .map(diag_stderr)
        .collect::<String>();
    Exit {
        code,
        stdout: String::new(),
        stderr,
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
        let src = "strake 1\n\nfn f(c: bool) -> i64\n  if c\n    ret 1\n  else\n    ret 2\n  end\n  ret 0\nend\n\ntest t\n  ret 0\nend\n";
        let tmp = std::env::temp_dir().join("strake_lib_lower70.strake");
        std::fs::write(&tmp, src).expect("write");
        let exit = test(tmp.to_str().unwrap(), Opts::default());
        assert_eq!(exit.code, 1);
        assert!(exit.stderr.contains("E_SEQ"));
    }

    #[test]
    fn fmt_eparse_exits_1() {
        let tmp = std::env::temp_dir().join("strake_fmt_eparse.strake");
        std::fs::write(&tmp, "fn f() -> unit\n  ret unit\nend\n").expect("write");
        let exit = fmt(tmp.to_str().unwrap());
        assert_eq!(exit.code, 1);
        assert!(exit.stderr.contains("E_PARSE"));
    }

    fn write_temp_strake(name: &str, src: &str) -> PathBuf {
        let path = std::env::temp_dir().join(name);
        std::fs::write(&path, src).expect("write temp strake");
        path
    }

    fn golden_repo() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    #[test]
    fn check_only_fn_keeps_export_and_matching_test() {
        let src = "strake 1\n\nfn a() -> i64\n  ret 1\nend\n\nfn b() -> i64\n  ret 2\nend\n\ntest t_a\n  v = call a()\n  assert.eq v 1\n  ret 0\nend\n\ntest t_b\n  v = call b()\n  assert.eq v 2\n  ret 0\nend\n";
        let path = write_temp_strake("strake_only_fn_ab.strake", src);
        let exit = check(
            path.to_str().unwrap(),
            Opts {
                only_fn: Some("a".into()),
                ..Opts::default()
            },
        );
        assert_eq!(exit.code, 0, "{}", exit.stderr);
    }

    #[test]
    fn check_only_fn_unknown_export_exit_2() {
        let src = "strake 1\n\nfn a() -> i64\n  ret 1\nend\n\ntest t\n  ret 0\nend\n";
        let path = write_temp_strake("strake_only_fn_unknown.strake", src);
        let exit = check(
            path.to_str().unwrap(),
            Opts {
                only_fn: Some("missing".into()),
                ..Opts::default()
            },
        );
        assert_eq!(exit.code, 2);
        assert!(exit.stderr.contains("unknown export"));
    }

    #[test]
    fn check_only_fn_private_export_exit_2() {
        let repo = golden_repo();
        let path = repo.join("golden/priv.strake");
        let exit = check(
            path.to_str().unwrap(),
            Opts {
                only_fn: Some("secret".into()),
                ..Opts::default()
            },
        );
        assert_eq!(exit.code, 2);
        assert!(exit.stderr.contains("unknown export"));
    }

    #[test]
    fn check_use_ok_with_blob_root() {
        let repo = golden_repo();
        let path = repo.join("golden/use_ok.strake");
        let exit = check(
            path.to_str().unwrap(),
            Opts {
                blob_root: Some(repo.join("golden/blob").to_string_lossy().into_owned()),
                ..Opts::default()
            },
        );
        assert_eq!(exit.code, 0, "{}", exit.stderr);
    }

    #[test]
    fn check_use_without_blob_root_e_use() {
        let repo = golden_repo();
        let path = repo.join("golden/use_ok.strake");
        let exit = check(path.to_str().unwrap(), Opts::default());
        assert_eq!(exit.code, 1);
        assert!(exit.stderr.contains("E_USE"));
    }

    #[test]
    fn check_use_blob_mismatch_e_use() {
        let repo = golden_repo();
        let path = repo.join("golden/use_ok.strake");
        let exit = check(
            path.to_str().unwrap(),
            Opts {
                blob_root: Some(repo.join("golden/blob-bad").to_string_lossy().into_owned()),
                ..Opts::default()
            },
        );
        assert_eq!(exit.code, 1);
        assert!(exit.stderr.contains("E_USE"));
    }

    #[test]
    fn check_add2_basic_ignores_blob_root_in_process() {
        let repo = golden_repo();
        let path = repo.join("golden/add2_basic.strake");
        let blob = repo.join("golden/blob").to_string_lossy().into_owned();
        let exit = check(
            path.to_str().unwrap(),
            Opts {
                blob_root: Some(blob),
                ..Opts::default()
            },
        );
        assert_eq!(exit.code, 0, "{}", exit.stderr);
        assert!(exit.stderr.is_empty());
    }

    #[test]
    fn compile_for_test_use_ok_with_blob_root() {
        let repo = golden_repo();
        let path = repo.join("golden/use_ok.strake");
        let program = compile_for_test(
            path.to_str().unwrap(),
            &Opts {
                blob_root: Some(repo.join("golden/blob").to_string_lossy().into_owned()),
                ..Opts::default()
            },
        )
        .expect("compile_for_test");
        assert!(!program.exports.is_empty());
    }

    #[test]
    fn test_command_passes_blob_root_to_check() {
        let repo = golden_repo();
        let path = repo.join("golden/add2_basic.strake");
        let exit = test(
            path.to_str().unwrap(),
            Opts {
                blob_root: Some(repo.join("golden/blob").to_string_lossy().into_owned()),
                ..Opts::default()
            },
        );
        assert_eq!(exit.code, 0, "{}", exit.stderr);
    }
}
