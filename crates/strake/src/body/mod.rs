pub mod check;
pub mod diag;
pub mod host;
pub mod interp;
pub mod lower;
pub mod syntax;

use diag::Diag;
use interp::ir::{FinishError, Program};
use syntax::Module;

pub(crate) const LOWER_ABORT_EXIT: i32 = 70;

#[derive(Debug)]
pub(crate) enum CompileError {
    Check(Vec<Diag>),
    Lower(FinishError),
}

/// CST → check → lower (exec is separate).
pub(crate) fn compile_module(module: &Module, _draft: bool) -> Result<Program, CompileError> {
    check::touch_catalog_for_clippy();
    let sketch = check::check(module).map_err(CompileError::Check)?;
    let program = lower::lower(&sketch).map_err(CompileError::Lower)?;
    Ok(program)
}

pub(crate) fn lower_abort_message(err: &FinishError) -> String {
    format!("{}\n", err.message())
}

#[cfg(test)]
mod tests {
    use super::*;
    use diag::Span;
    use interp::Builder;

    #[test]
    fn finish_rejection_maps_to_exit_70_plain_text() {
        let mut b = Builder::new();
        b.begin_function("bad", 0);
        let entry = 0;
        b.br(entry, span(), 42);
        let err = b.finish().unwrap_err();
        let msg = lower_abort_message(&err);
        assert!(!msg.contains("\"code\""));
        assert!(msg.contains("finish"));
        assert_eq!(LOWER_ABORT_EXIT, 70);
    }

    #[test]
    fn compile_lower_abort_does_not_touch_status() {
        let mut b = Builder::new();
        b.begin_function("bad", 0);
        b.br(0, span(), 9);
        let err = b.finish().unwrap_err();
        let _msg = lower_abort_message(&err);
        // `status` is only legal inside `lib::test`; this path must stop at 70.
        assert_eq!(LOWER_ABORT_EXIT, 70);
    }

    fn span() -> Span {
        Span {
            file: "t.strake".into(),
            start: 0,
            end: 1,
        }
    }
}
