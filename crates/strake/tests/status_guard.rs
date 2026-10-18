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
        match rel.to_str() {
            Some("body/diag.rs") => assert_diag_rs_status_rules(&content, rel),
            Some("lib.rs") => assert_lib_rs_status_rules(&content, rel),
            _ => assert_no_status_identifier(&content, rel),
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

fn assert_no_forbidden_diag_imports(content: &str, rel: &Path) {
    for (line_no, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if !trimmed.starts_with("use ") {
            continue;
        }
        if !trimmed.contains("body::diag") && !trimmed.contains("body::diag::") {
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

fn is_comment_line(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("//") || t.starts_with("///") || t.starts_with('*')
}

fn assert_no_status_identifier(content: &str, rel: &Path) {
    for (line_no, line) in content.lines().enumerate() {
        if is_comment_line(line) {
            continue;
        }
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

fn assert_diag_rs_status_rules(content: &str, rel: &Path) {
    let tests_start = content
        .find("#[cfg(test)]")
        .expect("diag.rs must have unit tests");
    for (line_no, line) in content.lines().enumerate() {
        if is_comment_line(line) {
            continue;
        }
        if !line_contains_status_identifier(line) {
            continue;
        }
        let byte_off = content
            .lines()
            .take(line_no)
            .map(|l| l.len() + 1)
            .sum::<usize>();
        let in_tests = byte_off >= tests_start;
        let is_definition = line.contains("fn status(");
        assert!(
            is_definition || in_tests,
            "{}:{}: status identifier only allowed on fn status or in diag tests: {}",
            rel.display(),
            line_no + 1,
            line.trim()
        );
    }
}

fn assert_lib_rs_status_rules(content: &str, rel: &Path) {
    let run_body = function_body(content, "pub fn run").expect("lib.rs must have pub fn run");
    let run_text = &content[run_body.range.clone()];
    assert!(
        !body_contains_status_identifier(run_text),
        "{}: pub fn run must not reference status",
        rel.display()
    );

    let test_body = function_body(content, "pub fn test").expect("lib.rs must have pub fn test");

    for (line_no, line) in content.lines().enumerate() {
        if is_comment_line(line) {
            continue;
        }
        if !line_contains_status_identifier(line) {
            continue;
        }
        let byte_off = content
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

fn body_contains_status_identifier(body: &str) -> bool {
    body.lines()
        .filter(|l| !is_comment_line(l))
        .any(line_contains_status_identifier)
}

fn line_contains_status_identifier(line: &str) -> bool {
    if line.contains("fn status(") {
        return false;
    }
    if line.contains("diag::status") || line.contains("body::diag::status") {
        return true;
    }
    if line.contains("::status") && line.contains("diag") {
        return true;
    }
    // call or binding to status: `status(`, `status;`, `status,`, `= status`
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
        let ident_after = after.is_ascii_alphanumeric() || after == b'_';
        if !ident_before
            && (after == b'('
                || after == b';'
                || after == b','
                || after == b')'
                || after.is_ascii_whitespace())
        {
            return true;
        }
        if !ident_before && !ident_after && after == b':' {
            // status: in use line already caught
        }
        search = &search[pos + 6..];
    }
    false
}
