//! Crate-wide guard: `diag::status` is only defined in `body/diag.rs` and referenced from `lib::test`.
use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn status_usage_guard() {
    let src_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for path in list_rs_files(&src_root) {
        let rel = path.strip_prefix(&src_root).unwrap();
        let content = fs::read_to_string(&path).expect("read source");
        assert_no_forbidden_diag_imports(&content, rel);
        let scrubbed = scrub_source(&content);
        match rel.to_str() {
            Some("body/diag.rs") => assert_diag_rs_status_rules(&scrubbed, rel),
            Some("lib.rs") => assert_lib_rs_status_rules(&content, &scrubbed, rel),
            _ => assert_no_status_identifier(&scrubbed, rel),
        }
    }
}

fn list_rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).expect("read src dir") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.is_dir() {
            out.extend(list_rs_files(&path));
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    out.sort();
    out
}

fn scrub_source(source: &str) -> String {
    let mut out = String::new();
    let mut i = 0usize;
    let bytes = source.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                out.push(' ');
                i += 1;
            }
            continue;
        }
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                out.push(' ');
                i += 1;
            }
            if i + 1 < bytes.len() {
                out.push(' ');
                out.push(' ');
                i += 2;
            }
            continue;
        }
        if bytes[i] == b'"' {
            out.push(' ');
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    out.push(' ');
                    out.push(' ');
                    i += 2;
                    continue;
                }
                if bytes[i] == b'"' {
                    out.push(' ');
                    i += 1;
                    break;
                }
                out.push(' ');
                i += 1;
            }
            continue;
        }
        if bytes[i] == b'\'' {
            out.push(' ');
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    out.push(' ');
                    out.push(' ');
                    i += 2;
                    continue;
                }
                if bytes[i] == b'\'' {
                    out.push(' ');
                    i += 1;
                    break;
                }
                out.push(' ');
                i += 1;
            }
            continue;
        }
        let ch = source[i..].chars().next().expect("char");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn assert_no_forbidden_diag_imports(content: &str, rel: &Path) {
    for (line_no, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if !trimmed.starts_with("use ") {
            continue;
        }
        if !trimmed.contains("body::diag") {
            continue;
        }
        assert!(
            !trimmed.contains('*'),
            "{}:{}: glob import from diag is forbidden: {}",
            rel.display(),
            line_no + 1,
            trimmed
        );
        assert!(
            !trimmed.contains(" as "),
            "{}:{}: renamed diag import is forbidden: {}",
            rel.display(),
            line_no + 1,
            trimmed
        );
    }
}

fn assert_no_status_identifier(scrubbed: &str, rel: &Path) {
    for (line_no, line) in scrubbed.lines().enumerate() {
        if line_contains_status_identifier(line) {
            panic!(
                "{}:{}: unexpected status identifier: {}",
                rel.display(),
                line_no + 1,
                line.trim()
            );
        }
    }
}

fn assert_diag_rs_status_rules(scrubbed: &str, rel: &Path) {
    let tests_start = scrubbed
        .find("#[cfg(test)]")
        .expect("diag.rs must have unit tests");
    for (line_no, line) in scrubbed.lines().enumerate() {
        if !line_contains_status_identifier(line) {
            continue;
        }
        let byte_off = scrubbed
            .lines()
            .take(line_no)
            .map(|l| l.len() + 1)
            .sum::<usize>();
        let in_tests = byte_off >= tests_start;
        let is_definition = line.trim_start().starts_with("pub(crate) fn status(")
            || line.trim_start().starts_with("fn status(");
        assert!(
            is_definition || in_tests,
            "{}:{}: status identifier only allowed on fn status or in diag tests: {}",
            rel.display(),
            line_no + 1,
            line.trim()
        );
    }
}

fn assert_lib_rs_status_rules(raw: &str, scrubbed: &str, rel: &Path) {
    let run_body = function_body(raw, "pub fn run").expect("lib.rs must have pub fn run");
    let run_scrub = &scrubbed[run_body.range.clone()];
    assert!(
        !scrubbed_contains_status(run_scrub),
        "{}: pub fn run must not reference status",
        rel.display()
    );

    let test_body = function_body(raw, "pub fn test").expect("lib.rs must have pub fn test");

    for (line_no, line) in scrubbed.lines().enumerate() {
        if !line_contains_status_identifier(line) {
            continue;
        }
        let byte_off = scrubbed
            .lines()
            .take(line_no)
            .map(|l| l.len() + 1)
            .sum::<usize>();
        let in_test_fn = test_body.range.contains(&byte_off);
        assert!(
            in_test_fn,
            "{}:{}: status identifier only allowed inside pub fn test: {}",
            rel.display(),
            line_no + 1,
            line.trim()
        );
    }
}

struct FnBody {
    range: std::ops::Range<usize>,
}

fn function_body(source: &str, sig: &str) -> Option<FnBody> {
    let start = source.find(sig)?;
    let after_sig = source[start..].find('{')? + start;
    let end = matching_brace_end(source, after_sig)?;
    Some(FnBody {
        range: after_sig..end + 1,
    })
}

fn matching_brace_end(source: &str, open: usize) -> Option<usize> {
    let mut depth = 0;
    for (idx, ch) in source[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + idx);
                }
            }
            _ => {}
        }
    }
    None
}

fn scrubbed_contains_status(body: &str) -> bool {
    body.lines().any(line_contains_status_identifier)
}

fn line_contains_status_identifier(line: &str) -> bool {
    if line.contains("diag::status") || line.contains("body::diag::status") {
        return true;
    }
    if line.contains("::status") && line.contains("diag") {
        return true;
    }
    let mut search = line;
    while let Some(pos) = search.find("status") {
        let before = if pos > 0 {
            search.as_bytes()[pos - 1]
        } else {
            b' '
        };
        let after = if pos + 6 < search.len() {
            search.as_bytes()[pos + 6]
        } else {
            b';'
        };
        let ident_before = before.is_ascii_alphanumeric() || before == b'_';
        if !ident_before
            && (after == b'('
                || after == b';'
                || after == b','
                || after == b')'
                || after.is_ascii_whitespace())
        {
            return true;
        }
        search = &search[pos + 6..];
    }
    false
}

fn expect_status_violation(scrubbed: &str) {
    let rel = Path::new("synthetic.rs");
    let result = std::panic::catch_unwind(|| assert_no_status_identifier(scrubbed, rel));
    assert!(
        result.is_err(),
        "expected status guard to reject synthetic snippet"
    );
}

#[cfg(test)]
mod violation_shapes {
    use super::{expect_status_violation, scrub_source};

    #[test]
    fn rustfmt_skip_run_with_diag_status_is_rejected() {
        let src = r#"#[rustfmt::skip]
fn run() {
    let _ = diag::status(&[]);
}
"#;
        expect_status_violation(&scrub_source(src));
    }

    #[test]
    fn rustfmt_skip_nested_helper_with_status_is_rejected() {
        let src = r#"#[rustfmt::skip]
fn run() {
    fn helper() {
        diag::status(&[]);
    }
    helper();
}
"#;
        expect_status_violation(&scrub_source(src));
    }

    #[test]
    fn rustfmt_skip_one_line_fn_status_wrapper_is_rejected() {
        let src = r#"#[rustfmt::skip]
fn run() {
    fn status(x: i32) { diag::status(&[]); }
    let _ = status(0);
}
"#;
        expect_status_violation(&scrub_source(src));
    }

    #[test]
    fn string_literal_status_is_not_a_false_positive() {
        let src = r#"let _ = "diag::status trap";
// trailing comment mentions status(1)
"#;
        let scrubbed = scrub_source(src);
        let rel = std::path::Path::new("synthetic.rs");
        super::assert_no_status_identifier(&scrubbed, rel);
    }
}
