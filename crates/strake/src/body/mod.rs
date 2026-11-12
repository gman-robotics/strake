pub mod carry;
pub mod check;
pub mod diag;
pub mod host;
pub mod interp;
pub mod lower;
pub mod syntax;

#[cfg(test)]
mod ssa_invariant;

use interp::Program;
use lower::LowerError;
use syntax::Module;

pub(crate) const LOWER_ABORT_EXIT: i32 = 70;

#[derive(Debug)]
pub(crate) enum CompileError {
    Check(check::CheckFailure),
    Lower(LowerError),
}

/// CST → check → lower (exec is separate).
pub(crate) fn compile_module(module: &Module, draft: bool) -> Result<Program, CompileError> {
    let sketch = check::check(module, draft).map_err(CompileError::Check)?;
    let program = lower::lower(&sketch).map_err(CompileError::Lower)?;
    #[cfg(debug_assertions)]
    {
        for func in &program.functions {
            interp::ir::verify_function_ssa_public(func)
                .map_err(|e| CompileError::Lower(LowerError::Finish(e)))?;
        }
    }
    Ok(program)
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
    fn catalog_check_failures_do_not_enter_exec() {
        let src = include_str!("../../../../golden/shadow.strake");
        let tmp = std::env::temp_dir().join("strake_t4_shadow_exec.strake");
        std::fs::write(&tmp, src).expect("write");
        let before = crate::exec_entries_on_this_thread();
        let exit = strake_test(tmp.to_str().unwrap(), Opts::default());
        assert_eq!(exit.code, 1);
        assert_eq!(crate::exec_entries_on_this_thread(), before);

        let src = include_str!("../../../../golden/priv_call.strake");
        let tmp = std::env::temp_dir().join("strake_t4_priv_call_exec.strake");
        std::fs::write(&tmp, src).expect("write");
        let before = crate::exec_entries_on_this_thread();
        let exit = strake_test(tmp.to_str().unwrap(), Opts::default());
        assert_eq!(exit.code, 3);
        assert_eq!(crate::exec_entries_on_this_thread(), before);
    }

    /// `forall_rev` is check `E_FORALL` before any export runs. The count is
    /// thread-local, so other tests running `strake::test` in parallel do not
    /// move it. A passing property on this thread then adds one.
    #[test]
    fn forall_rev_check_does_not_enter_exec() {
        let rev_src = include_str!("../../../../golden/forall_rev.strake");
        let rev_path = std::env::temp_dir().join("strake_forall_rev_exec.strake");
        std::fs::write(&rev_path, rev_src).expect("write");
        let before = crate::exec_entries_on_this_thread();
        let blocked = strake_test(rev_path.to_str().unwrap(), Opts::default());
        assert_eq!(blocked.code, 1, "{}", blocked.stderr);
        assert!(
            blocked.stderr.contains("\"E_FORALL\""),
            "{}",
            blocked.stderr
        );
        let after_check = crate::exec_entries_on_this_thread();
        assert_eq!(after_check, before);

        let ok_src = include_str!("../../../../golden/forall_ok.strake");
        let ok_path = std::env::temp_dir().join("strake_forall_ok_exec.strake");
        std::fs::write(&ok_path, ok_src).expect("write");
        let ran = strake_test(ok_path.to_str().unwrap(), Opts::default());
        assert_eq!(ran.code, 0, "{}", ran.stderr);
        assert_eq!(crate::exec_entries_on_this_thread(), after_check + 1);
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
    fn loop_in_fn_body_compiles() {
        let src = "strake 1\n\nfn bad() -> i64\n  loop x = 0\n    break\n  end\n  ret x\nend\n\ntest t\n  ret 0\nend\n";
        let module = syntax::parse(src, "t.strake").expect("parse");
        assert!(compile_module(&module, false).is_ok());
    }
}
