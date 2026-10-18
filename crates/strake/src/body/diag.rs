//! Catalog-backed diagnostics and the test-channel exit aggregator.

use std::fmt::Write as _;
use std::io::Write;

/// Diagnostic channel kind (catalog `kinds`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Check,
    Trap,
    Result,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Check => "check",
            Kind::Trap => "trap",
            Kind::Result => "result",
        }
    }
}

/// Stable catalog fault / result code. `E_CYCLE` is reserved and has no constructor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Code {
    EParse,
    EType,
    EShadow,
    EForall,
    EMatch,
    EIfPhi,
    EHole,
    ERegion,
    EUse,
    EImport,
    EOob,
    EStack,
    EDiv0,
    EFuel,
    ELower,
    ETest,
    ERefused,
}

impl Code {
    pub const ALL: [Code; 17] = [
        Code::EParse,
        Code::EType,
        Code::EShadow,
        Code::EForall,
        Code::EMatch,
        Code::EIfPhi,
        Code::EHole,
        Code::ERegion,
        Code::EUse,
        Code::EImport,
        Code::EOob,
        Code::EStack,
        Code::EDiv0,
        Code::EFuel,
        Code::ELower,
        Code::ETest,
        Code::ERefused,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Code::EParse => "E_PARSE",
            Code::EType => "E_TYPE",
            Code::EShadow => "E_SHADOW",
            Code::EForall => "E_FORALL",
            Code::EMatch => "E_MATCH",
            Code::EIfPhi => "E_IF_PHI",
            Code::EHole => "E_HOLE",
            Code::ERegion => "E_REGION",
            Code::EUse => "E_USE",
            Code::EImport => "E_IMPORT",
            Code::EOob => "E_OOB",
            Code::EStack => "E_STACK",
            Code::EDiv0 => "E_DIV0",
            Code::EFuel => "E_FUEL",
            Code::ELower => "E_LOWER",
            Code::ETest => "E_TEST",
            Code::ERefused => "E_REFUSED",
        }
    }

    pub fn msg(self) -> &'static str {
        match self {
            Code::EParse => "token stream is not v0 A",
            Code::EType => {
                "operand type, arity, compare family, or eq on list/record/sum"
            }
            Code::EShadow => {
                "a source name has a second binding site in the same function"
            }
            Code::EForall => {
                "forall span empty, reversed, does not fit i64, or bounds not static i64"
            }
            Code::EMatch => "match is not exhaustive",
            Code::EIfPhi => "a name used after if is missing from one arm",
            Code::EHole => {
                "ship module rejects a hole before lower; --draft execution traps on hole"
            }
            Code::ERegion => "load/store used without an import region mem : region",
            Code::EUse => {
                "use hash missing, or not equal to blake3 of the formatted text"
            }
            Code::EImport => {
                "unknown callee (check); missing host binding or negative fuel_cost at instantiate (trap)"
            }
            Code::EOob => "region byte range outside pages * 65536",
            Code::EStack => "call would exceed 1024 frames",
            Code::EDiv0 => "integer divisor 0, or signed MIN / -1 on i32 or i64",
            Code::EFuel => "remaining fuel less than the next instruction's cost",
            Code::ELower => "for desugar observed list.get Err while index < len",
            Code::ETest => "assert.eq mismatch",
            Code::ERefused => "test or property executed ret 77",
        }
    }
}

/// Half-open byte span in a source file (§2.8).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub file: String,
    pub start: u32,
    pub end: u32,
}

/// One catalog diagnostic before rendering.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diag {
    pub code: Code,
    pub kind: Kind,
    pub span: Span,
    pub want: Option<String>,
    pub got: Option<String>,
    pub patch: Option<String>,
}

impl Diag {
    pub fn msg(&self) -> &'static str {
        self.code.msg()
    }

    /// JSON Lines object: catalog field order; absent optionals omitted.
    pub fn write_json_line(&self, out: &mut impl Write) -> std::io::Result<()> {
        let mut line = String::new();
        line.push('{');
        write_json_str_field(&mut line, "code", self.code.as_str(), true);
        write_json_str_field(&mut line, "kind", self.kind.as_str(), false);
        write_json_str_field(&mut line, "msg", self.msg(), false);
        write_json_span_field(&mut line, &self.span, false);
        if let Some(want) = &self.want {
            write_json_str_field(&mut line, "want", want, false);
        }
        if let Some(got) = &self.got {
            write_json_str_field(&mut line, "got", got, false);
        }
        if let Some(patch) = &self.patch {
            write_json_str_field(&mut line, "patch", patch, false);
        }
        line.push('}');
        line.push('\n');
        out.write_all(line.as_bytes())
    }
}

fn write_json_str_field(buf: &mut String, key: &str, value: &str, first: bool) {
    if !first {
        buf.push(',');
    }
    buf.push('"');
    buf.push_str(key);
    buf.push_str("\":");
    json_escape_str(buf, value);
}

fn write_json_span_field(buf: &mut String, span: &Span, first: bool) {
    if !first {
        buf.push(',');
    }
    buf.push_str("\"span\":{");
    write_json_str_field(buf, "file", &span.file, true);
    write_json_num_field(buf, "start", span.start, false);
    write_json_num_field(buf, "end", span.end, false);
    buf.push('}');
}

fn write_json_num_field(buf: &mut String, key: &str, value: u32, first: bool) {
    if !first {
        buf.push(',');
    }
    let _ = write!(buf, "\"{key}\":{value}");
}

fn json_escape_str(buf: &mut String, s: &str) {
    buf.push('"');
    for ch in s.chars() {
        match ch {
            '"' => buf.push_str("\\\""),
            '\\' => buf.push_str("\\\\"),
            '\n' => buf.push_str("\\n"),
            '\r' => buf.push_str("\\r"),
            '\t' => buf.push_str("\\t"),
            c if c.is_control() => {
                let _ = write!(buf, "\\u{:04x}", c as u32);
            }
            c => buf.push(c),
        }
    }
    buf.push('"');
}

/// Per test/property export outcome fed into [`status`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExportOutcome {
    pub trapped: bool,
    pub ret: i64,
}

/// Test-channel exit code only (`run` does not call this).
///
/// Precedence (catalog `cli.precedence`, SPEC "Tests"):
/// any trap -> 2; else any 77 -> 77; else any nonzero test -> 1; else 0
pub fn status(exports: &[ExportOutcome]) -> i32 {
    if exports.iter().any(|e| e.trapped) {
        return 2;
    }
    if exports.iter().any(|e| e.ret == 77) {
        return 77;
    }
    if exports.iter().any(|e| e.ret != 0) {
        return 1;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_trap_beats_77() {
        let exports = [
            ExportOutcome {
                trapped: true,
                ret: 0,
            },
            ExportOutcome {
                trapped: false,
                ret: 77,
            },
        ];
        assert_eq!(status(&exports), 2);
    }

    #[test]
    fn status_77_beats_nonzero() {
        let exports = [
            ExportOutcome {
                trapped: false,
                ret: 77,
            },
            ExportOutcome {
                trapped: false,
                ret: 2,
            },
        ];
        assert_eq!(status(&exports), 77);
    }

    #[test]
    fn status_ret_2_is_1() {
        let exports = [ExportOutcome {
            trapped: false,
            ret: 2,
        }];
        assert_eq!(status(&exports), 1);
    }

    #[test]
    fn status_all_zero() {
        let exports = [
            ExportOutcome {
                trapped: false,
                ret: 0,
            },
            ExportOutcome {
                trapped: false,
                ret: 0,
            },
        ];
        assert_eq!(status(&exports), 0);
    }

    #[test]
    fn json_line_field_order_and_optionals() {
        let diag = Diag {
            code: Code::ETest,
            kind: Kind::Result,
            span: Span {
                file: "t.strake".into(),
                start: 1,
                end: 5,
            },
            want: Some("14".into()),
            got: Some("16".into()),
            patch: None,
        };
        let mut buf = Vec::new();
        diag.write_json_line(&mut buf).unwrap();
        let line = String::from_utf8(buf).unwrap();
        assert_eq!(
            line,
            "{\"code\":\"E_TEST\",\"kind\":\"result\",\"msg\":\"assert.eq mismatch\",\
\"span\":{\"file\":\"t.strake\",\"start\":1,\"end\":5},\"want\":\"14\",\"got\":\"16\"}\n"
        );
        assert!(!line.contains("patch"));
    }

    #[test]
    fn e_hole_kind_is_one_string() {
        let check = Diag {
            code: Code::EHole,
            kind: Kind::Check,
            span: Span {
                file: "h.strake".into(),
                start: 0,
                end: 1,
            },
            want: None,
            got: None,
            patch: None,
        };
        let trap = Diag {
            code: Code::EHole,
            kind: Kind::Trap,
            span: Span {
                file: "h.strake".into(),
                start: 0,
                end: 1,
            },
            want: None,
            got: None,
            patch: None,
        };
        let mut c = Vec::new();
        let mut t = Vec::new();
        check.write_json_line(&mut c).unwrap();
        trap.write_json_line(&mut t).unwrap();
        assert!(c.windows(2).all(|w| w != b"[[") && !c.contains(&b'['));
        assert!(String::from_utf8(c).unwrap().contains("\"kind\":\"check\""));
        assert!(String::from_utf8(t).unwrap().contains("\"kind\":\"trap\""));
    }

    #[test]
    fn catalog_msgs_match_include() {
        const CATALOG: &str = include_str!("../../../../errors/catalog.json");
        for code in Code::ALL {
            assert!(
                catalog_entry_has_msg(CATALOG, code),
                "missing catalog entry for {}",
                code.as_str()
            );
            let msg = extract_catalog_msg(CATALOG, code.as_str());
            assert_eq!(
                msg.as_deref(),
                Some(code.msg()),
                "msg drift for {}",
                code.as_str()
            );
        }
        assert!(
            CATALOG.contains("\"E_CYCLE\""),
            "catalog should mention reserved E_CYCLE"
        );
        assert!(
            !CATALOG.contains("cyclic value") || Code::ALL.iter().all(|c| c.as_str() != "E_CYCLE"),
            "E_CYCLE must not be constructable as Code"
        );
    }

    fn catalog_entry_has_msg(catalog: &str, code: Code) -> bool {
        catalog.contains(&format!("\"code\": \"{}\"", code.as_str()))
    }

    fn extract_catalog_msg(catalog: &str, code: &str) -> Option<String> {
        let pattern = format!("\"code\": \"{code}\"");
        let start = catalog.find(&pattern)?;
        let rest = &catalog[start..];
        let msg_key = rest.find("\"msg\": \"")?;
        let after = &rest[msg_key + 8..];
        let end = after.find('"')?;
        Some(after[..end].to_string())
    }
}
