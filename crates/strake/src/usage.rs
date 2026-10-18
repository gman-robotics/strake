//! Shared CLI usage strings (single source for lib and main).

/// Usage line for `strake check` (includes `[--blob-root DIR]` for T8).
pub const CHECK_USAGE: &str =
    "usage: strake check [--draft] [--fn NAME] [--blob-root DIR] <file|dir>";
