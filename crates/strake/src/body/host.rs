//! Host bindings for imports (MVP: empty host only).

use super::interp::Value;

/// Crate-private host surface for `Machine::call`.
pub(crate) trait Host {
    #[allow(dead_code)]
    fn resolve_import(&self, path: &str) -> Option<ImportFn>;

    /// Extra fuel for one call to the import at `path` (SPEC "Fuel"): default 0,
    /// must be >= 0. A negative cost is `E_IMPORT` at instantiate and at call.
    fn fuel_cost(&self, _path: &str) -> i64 {
        0
    }
}

#[allow(dead_code)]
pub(crate) type ImportFn = fn(&mut super::interp::Machine, &[Value]) -> Result<Value, ()>;

/// CLI default: no imports resolve.
pub(crate) struct EmptyHost;

impl Host for EmptyHost {
    fn resolve_import(&self, _path: &str) -> Option<ImportFn> {
        None
    }
}
