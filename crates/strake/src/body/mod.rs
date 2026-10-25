pub mod check;
pub mod diag;
pub mod host;
pub mod interp;
pub mod lower;
pub mod syntax;

use diag::Diag;
use interp::Program;
use lower::LowerError;
use syntax::Module;

pub(crate) const LOWER_ABORT_EXIT: i32 = 70;

#[derive(Debug)]
pub(crate) enum CompileError {
    Check(Vec<Diag>),
    Lower(LowerError),
}

/// CST → check → lower (exec is separate).
pub(crate) fn compile_module(module: &Module, draft: bool) -> Result<Program, CompileError> {
    let sketch = check::check(module, draft).map_err(CompileError::Check)?;
    let program = lower::lower(&sketch).map_err(CompileError::Lower)?;
    Ok(program)
}

#[cfg(test)]
pub(crate) static EXEC_ENTERED: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

#[cfg(test)]
pub(crate) fn exec_entered_count() -> usize {
    EXEC_ENTERED.load(std::sync::atomic::Ordering::SeqCst)
}

#[cfg(test)]
pub(crate) fn reset_exec_entered() {
    EXEC_ENTERED.store(0, std::sync::atomic::Ordering::SeqCst);
}

pub(crate) fn lower_abort_message(err: &LowerError) -> String {
    err.message()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body::syntax;
    use crate::{test as strake_test, Opts};

    #[test]
    fn unsupported_sub_lowering_returns_exit_70() {
        let src =
            "strake 1\n\nfn f() -> i64\n  x = sub i64 8 3\n  ret x\nend\n\ntest t\n  ret 0\nend\n";
        let tmp = std::env::temp_dir().join("strake_sub_lower.strake");
        std::fs::write(&tmp, src).expect("write");
        let exit = strake_test(tmp.to_str().unwrap(), Opts::default());
        assert_eq!(exit.code, LOWER_ABORT_EXIT);
        assert!(!exit.stderr.contains("\"code\""));
        assert!(exit.stderr.contains("lower:"));
    }

    #[test]
    fn catalog_check_failures_do_not_enter_exec() {
        reset_exec_entered();
        let src = include_str!("../../../../golden/shadow.strake");
        let tmp = std::env::temp_dir().join("strake_t4_shadow_exec.strake");
        std::fs::write(&tmp, src).expect("write");
        let exit = strake_test(tmp.to_str().unwrap(), Opts::default());
        assert_eq!(exit.code, 1);
        assert_eq!(exec_entered_count(), 0);

        reset_exec_entered();
        let src = include_str!("../../../../golden/priv_call.strake");
        let tmp = std::env::temp_dir().join("strake_t4_priv_call_exec.strake");
        std::fs::write(&tmp, src).expect("write");
        let exit = strake_test(tmp.to_str().unwrap(), Opts::default());
        assert_eq!(exit.code, 3);
        assert_eq!(exec_entered_count(), 0);
    }

    #[test]
    fn priv_fn_omitted_from_exports() {
        let src = include_str!("../../../../golden/priv.strake");
        let module = syntax::parse(src, "golden/priv.strake").expect("parse");
        let program = compile_module(&module, false).expect("compile");
        assert!(program.exports.iter().any(|e| e.name == "visible"));
        assert!(!program.exports.iter().any(|e| e.name == "secret"));
    }

    #[test]
    fn property_export_named_property_dot_name() {
        let src = "strake 1\n\nproperty smoke\n  ret 0\nend\n";
        let module = syntax::parse(src, "t.strake").expect("parse");
        let program = compile_module(&module, false).expect("compile");
        assert!(program
            .exports
            .iter()
            .any(|e| e.name == "property.smoke" && e.is_test));
    }

    #[test]
    fn unsupported_fn_call_lowering_returns_exit_70() {
        let src = "strake 1\n\nfn add2(a: i64, b: i64) -> i64\n  x = add i64 a b\n  y = mul i64 x 2\n  ret y\nend\n\nfn twice(a: i64) -> i64\n  r = call add2(a, a)\n  ret r\nend\n\ntest t\n  ret 0\nend\n";
        let tmp = std::env::temp_dir().join("strake_twice_lower.strake");
        std::fs::write(&tmp, src).expect("write");
        let exit = strake_test(tmp.to_str().unwrap(), Opts::default());
        assert_eq!(exit.code, LOWER_ABORT_EXIT);
        assert!(!exit.stderr.contains("\"code\""));
    }
}
