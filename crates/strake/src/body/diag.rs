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

/// Stable catalog fault / result code. Reserved catalog codes (e.g. `E_CYCLE`) have no variant.
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
    /// Every constructable catalog code (keep in sync with [`Code`] variants).
    pub(crate) fn constructable() -> [Code; 17] {
        [
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
        ]
    }

    /// Map a catalog `code` string to a constructable [`Code`]. Reserved names return `None`.
    pub fn from_catalog_name(name: &str) -> Option<Code> {
        match name {
            "E_PARSE" => Some(Code::EParse),
            "E_TYPE" => Some(Code::EType),
            "E_SHADOW" => Some(Code::EShadow),
            "E_FORALL" => Some(Code::EForall),
            "E_MATCH" => Some(Code::EMatch),
            "E_IF_PHI" => Some(Code::EIfPhi),
            "E_HOLE" => Some(Code::EHole),
            "E_REGION" => Some(Code::ERegion),
            "E_USE" => Some(Code::EUse),
            "E_IMPORT" => Some(Code::EImport),
            "E_OOB" => Some(Code::EOob),
            "E_STACK" => Some(Code::EStack),
            "E_DIV0" => Some(Code::EDiv0),
            "E_FUEL" => Some(Code::EFuel),
            "E_LOWER" => Some(Code::ELower),
            "E_TEST" => Some(Code::ETest),
            "E_REFUSED" => Some(Code::ERefused),
            "E_CYCLE" => None,
            _ => None,
        }
    }

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

/// Per test/property export outcome fed into the test-channel status aggregator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExportOutcome {
    pub trapped: bool,
    pub ret: i64,
}

/// Test-channel exit code only (`run` must not call this).
///
/// Precedence (catalog `cli.precedence`, SPEC "Tests"):
/// any trap -> 2; else any 77 -> 77; else any nonzero test -> 1; else 0
pub(crate) fn status(exports: &[ExportOutcome]) -> i32 {
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
            ExportOutcome {
                trapped: true,
                ret: 77,
            },
        ];
        assert_eq!(status(&exports), 2);
    }

    #[test]
    fn status_trap_alone_with_ret_77_is_2() {
        let exports = [ExportOutcome {
            trapped: true,
            ret: 77,
        }];
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
    fn e_hole_json_omits_absent_optionals() {
        let diag = Diag {
            code: Code::EHole,
            kind: Kind::Check,
            span: Span {
                file: "h.strake".into(),
                start: 0,
                end: 4,
            },
            want: None,
            got: None,
            patch: None,
        };
        let mut buf = Vec::new();
        diag.write_json_line(&mut buf).unwrap();
        let line = String::from_utf8(buf).unwrap();
        assert!(!line.contains("want"));
        assert!(!line.contains("got"));
        assert!(!line.contains("patch"));
        assert!(!line.contains("null"));
    }

    #[test]
    fn json_escapes_strings() {
        let diag = Diag {
            code: Code::ETest,
            kind: Kind::Result,
            span: Span {
                file: "a\"b\\c\n.strake".into(),
                start: 0,
                end: 1,
            },
            want: Some("w\"o\n".into()),
            got: Some("g\\t".into()),
            patch: None,
        };
        let mut buf = Vec::new();
        diag.write_json_line(&mut buf).unwrap();
        let line = String::from_utf8(buf).unwrap();
        assert!(line.contains("a\\\"b\\\\c\\n.strake"));
        assert!(line.contains("w\\\"o\\n"));
        assert!(line.contains("g\\\\t"));
    }

    #[test]
    fn e_cycle_not_constructable() {
        assert_eq!(Code::from_catalog_name("E_CYCLE"), None);
    }

    #[test]
    fn catalog_msgs_match_include() {
        const CATALOG: &str = include_str!("../../../../errors/catalog.json");
        let entries = parse_catalog_codes(CATALOG);
        let non_reserved: Vec<_> = entries.iter().filter(|e| !e.reserved).collect();
        let constructable = Code::constructable();

        assert_eq!(
            non_reserved.len(),
            constructable.len(),
            "catalog non-reserved count must match Code variants"
        );

        for entry in &non_reserved {
            let code = Code::from_catalog_name(&entry.code)
                .unwrap_or_else(|| panic!("catalog code {} has no Code constructor", entry.code));
            assert_eq!(code.msg(), entry.msg, "msg drift for {}", entry.code);
        }

        for code in constructable {
            let name = exhaustive_catalog_name(code);
            assert_eq!(name, code.as_str());
            let entry = non_reserved
                .iter()
                .find(|e| e.code == name)
                .unwrap_or_else(|| panic!("Code {} missing from catalog", name));
            assert_eq!(code.msg(), entry.msg);
            assert!(
                !entries.iter().any(|e| e.code == name && e.reserved),
                "{name} must not be a reserved catalog row"
            );
        }

        let cycle = entries
            .iter()
            .find(|e| e.code == "E_CYCLE")
            .expect("catalog must list E_CYCLE");
        assert!(cycle.reserved, "E_CYCLE must stay reserved in catalog");
        assert_eq!(Code::from_catalog_name("E_CYCLE"), None);
    }

    /// Exhaustive over [`Code`] — no wildcard; a new variant is a compile error here.
    fn exhaustive_catalog_name(code: Code) -> &'static str {
        match code {
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

    struct CatalogCodeEntry {
        code: String,
        msg: String,
        reserved: bool,
    }

    fn parse_catalog_codes(json: &str) -> Vec<CatalogCodeEntry> {
        let codes_key = json.find("\"codes\"").expect("catalog missing codes array");
        let array_start = json[codes_key..].find('[').expect("codes array start") + codes_key;
        let mut out = Vec::new();
        let bytes = json.as_bytes();
        let mut i = array_start + 1;
        while i < bytes.len() {
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i >= bytes.len() || bytes[i] == b']' {
                break;
            }
            if bytes[i] != b'{' {
                i += 1;
                continue;
            }
            let obj_start = i;
            let obj_end = find_matching_brace(json, obj_start);
            let obj = &json[obj_start..=obj_end];
            let code = parse_json_string_field(obj, "code").expect("code field");
            let msg = parse_json_string_field(obj, "msg").expect("msg field");
            let reserved = obj.contains("\"reserved\": true");
            out.push(CatalogCodeEntry {
                code,
                msg,
                reserved,
            });
            i = obj_end + 1;
        }
        out
    }

    fn find_matching_brace(s: &str, start: usize) -> usize {
        let mut depth = 0;
        for (idx, ch) in s[start..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return start + idx;
                    }
                }
                _ => {}
            }
        }
        panic!("unclosed object");
    }

    fn parse_json_string_field(obj: &str, key: &str) -> Option<String> {
        let pattern = format!("\"{key}\": \"");
        let start = obj.find(&pattern)? + pattern.len();
        let rest = &obj[start..];
        let mut out = String::new();
        let mut escaped = false;
        for ch in rest.chars() {
            if escaped {
                out.push(ch);
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == '"' {
                return Some(out);
            }
            out.push(ch);
        }
        None
    }
}
