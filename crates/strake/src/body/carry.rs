//! Loop carry and `if` join plan (syntax only). Built by `check`, consumed by `lower`.

use std::collections::HashMap;

use super::diag::Span;
use super::syntax::{Operand, Stmt};

/// Stable identity for a loop or `if` join site in the carry plan (assigned at check time).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SiteId(pub u32);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CarryPlan {
    pub loops: Vec<LoopSite>,
    pub joins: Vec<JoinSite>,
    pub matches: Vec<MatchSite>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MatchSite {
    pub id: SiteId,
    pub span: Span,
    pub scrutinee: String,
    /// `result` is `["ok", "err"]`. A user sum is declaration order.
    pub tags: Vec<String>,
    pub arms: Vec<MatchArmSupply>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MatchArmSupply {
    pub tag_index: u32,
    /// This arm executes a `Br` to the join.
    pub reaches: bool,
    /// Names on that `Br`. Empty when `reaches` is false.
    pub delivered: Vec<String>,
}

pub fn match_site_by_span<'a>(plan: &'a CarryPlan, span: &Span) -> Option<&'a MatchSite> {
    plan.matches
        .iter()
        .find(|s| s.span.file == span.file && s.span.start == span.start && s.span.end == span.end)
}

/// Phi names at a match join: intersection of `delivered` over reaching arms.
pub fn match_phi_names(site: &MatchSite) -> Vec<String> {
    let reaching: Vec<&MatchArmSupply> = site.arms.iter().filter(|a| a.reaches).collect();
    if reaching.is_empty() {
        return Vec::new();
    }
    let mut names: Vec<String> = reaching[0].delivered.clone();
    for arm in reaching.iter().skip(1) {
        names.retain(|n| arm.delivered.iter().any(|d| d == n));
    }
    names.sort();
    names.dedup();
    names
}

pub fn loop_site_by_span<'a>(plan: &'a CarryPlan, span: &Span) -> Option<&'a LoopSite> {
    plan.loops
        .iter()
        .find(|s| s.span.file == span.file && s.span.start == span.start && s.span.end == span.end)
}

pub fn join_site_by_span<'a>(plan: &'a CarryPlan, span: &Span) -> Option<&'a JoinSite> {
    plan.joins
        .iter()
        .find(|s| s.span.file == span.file && s.span.start == span.start && s.span.end == span.end)
}

#[derive(Clone, Debug, PartialEq)]
pub struct LoopSite {
    pub id: SiteId,
    pub span: Span,
    pub params: Vec<String>,
    pub backedges: Vec<BackEdge>,
    pub breaks: Vec<BreakEdge>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BackEdge {
    pub span: Span,
    pub operands: Vec<Operand>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BreakEdge {
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct JoinSite {
    pub id: SiteId,
    pub span: Span,
    pub names: Vec<NameSupply>,
}

/// Names `lower` must bind at an `if` join (S5 keeps only the first shared assign name).
pub fn if_join_bind_names(
    plan: &CarryPlan,
    site: &JoinSite,
    then: &[Stmt],
    else_body: &[Stmt],
) -> Vec<String> {
    let has_loop_join = site.names.iter().any(publishes_loop_join);
    let mut out = if has_loop_join {
        let mut names = Vec::new();
        if let Some(n) = shared_assign_join_name(then, else_body) {
            names.push(n);
        }
        for supply in &site.names {
            if publishes_loop_join(supply) && !names.iter().any(|x| x == &supply.name) {
                names.push(supply.name.clone());
            }
        }
        names
    } else {
        shared_assign_join_name(then, else_body)
            .map(|n| vec![n])
            .unwrap_or_default()
    };
    append_match_published_if_names(plan, then, else_body, &mut out);
    out
}

fn append_match_published_if_names(
    plan: &CarryPlan,
    then: &[Stmt],
    else_body: &[Stmt],
    out: &mut Vec<String>,
) {
    let then_match = match_published_names_in_arm(then, plan);
    let else_match = match_published_names_in_arm(else_body, plan);
    let then_delivers = if_arm_delivered_names(then, plan);
    let else_delivers = if_arm_delivered_names(else_body, plan);
    for name in then_match.union(&else_match) {
        if then_delivers.contains(name) && else_delivers.contains(name) && !out.contains(name) {
            out.push(name.clone());
        }
    }
}

fn match_published_names_in_arm(
    body: &[Stmt],
    plan: &CarryPlan,
) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    for stmt in body {
        if let Stmt::Match { span, .. } = stmt {
            if let Some(site) = match_site_by_span(plan, span) {
                out.extend(match_phi_names(site));
            }
        }
    }
    out
}

fn if_arm_delivered_names(body: &[Stmt], plan: &CarryPlan) -> std::collections::HashSet<String> {
    let mut names = std::collections::HashSet::new();
    for stmt in body {
        match stmt {
            Stmt::Assign { name, .. } => {
                names.insert(name.clone());
            }
            Stmt::If {
                span,
                then_body,
                else_body,
                ..
            } => {
                if let Some(site) = join_site_by_span(plan, span) {
                    for n in if_join_bind_names(plan, site, then_body, else_body) {
                        names.insert(n);
                    }
                }
            }
            Stmt::Match { span, .. } => {
                if let Some(site) = match_site_by_span(plan, span) {
                    for n in match_phi_names(site) {
                        names.insert(n);
                    }
                }
            }
            _ => {}
        }
    }
    names
}

/// Names delivered on the `Br` leaving a match arm body.
pub fn match_arm_delivered(body: &[Stmt], plan: &CarryPlan) -> Vec<String> {
    let mut names: std::collections::HashSet<String> = std::collections::HashSet::new();
    for stmt in body {
        match stmt {
            Stmt::Assign { name, .. } => {
                names.insert(name.clone());
            }
            Stmt::If {
                span,
                then_body,
                else_body,
                ..
            } => {
                if let Some(site) = join_site_by_span(plan, span) {
                    for n in if_join_bind_names(plan, site, then_body, else_body) {
                        names.insert(n);
                    }
                }
            }
            Stmt::Match { span, .. } => {
                if let Some(site) = match_site_by_span(plan, span) {
                    for n in match_phi_names(site) {
                        names.insert(n);
                    }
                }
            }
            _ => {}
        }
    }
    let mut out: Vec<String> = names.into_iter().collect();
    out.sort();
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameSupply {
    pub name: String,
    pub then_arm: ArmSrc,
    pub else_arm: ArmSrc,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArmSrc {
    Assign,
    LoopExit,
    Absent,
}

/// Direct top-level statements of one `if` arm (not nested `if` bodies).
pub fn direct_arm_sources(body: &[Stmt]) -> HashMap<String, ArmSrc> {
    let mut out = HashMap::new();
    for stmt in body {
        match stmt {
            Stmt::Assign { name, .. } => {
                if out.get(name) == Some(&ArmSrc::LoopExit) {
                    continue;
                }
                if out.contains_key(name) {
                    continue;
                }
                out.insert(name.clone(), ArmSrc::Assign);
            }
            Stmt::Loop { params, .. } => {
                for p in params {
                    if out.contains_key(&p.name) {
                        continue;
                    }
                    out.insert(p.name.clone(), ArmSrc::LoopExit);
                }
            }
            _ => {}
        }
    }
    for stmt in body {
        if let Stmt::Assign { name, .. } = stmt {
            if out.get(name) == Some(&ArmSrc::LoopExit) {
                // shadow: assign and loop on same name — arm does not supply `name`.
                out.remove(name);
            }
        }
        if let Stmt::Loop { params, .. } = stmt {
            for p in params {
                if body
                    .iter()
                    .any(|s| matches!(s, Stmt::Assign { name, .. } if name == &p.name))
                {
                    out.remove(&p.name);
                }
            }
        }
    }
    out
}

pub fn join_name_supplies(then: &[Stmt], else_body: &[Stmt]) -> Vec<NameSupply> {
    let t = direct_arm_sources(then);
    let e = direct_arm_sources(else_body);
    let mut names: Vec<String> = t.keys().chain(e.keys()).cloned().collect();
    names.sort();
    names.dedup();
    let mut out = Vec::new();
    for name in names {
        let then_arm = t.get(&name).copied().unwrap_or(ArmSrc::Absent);
        let else_arm = e.get(&name).copied().unwrap_or(ArmSrc::Absent);
        out.push(NameSupply {
            name,
            then_arm,
            else_arm,
        });
    }
    out
}

/// Loop-join publish rule from the sketch.
pub fn publishes_loop_join(supply: &NameSupply) -> bool {
    let then_ok = matches!(supply.then_arm, ArmSrc::Assign | ArmSrc::LoopExit);
    let else_ok = matches!(supply.else_arm, ArmSrc::Assign | ArmSrc::LoopExit);
    let any_loop = supply.then_arm == ArmSrc::LoopExit || supply.else_arm == ArmSrc::LoopExit;
    then_ok && else_ok && any_loop
}

/// Classic two-arm assign join (first shared top-level assign name).
pub fn shared_assign_join_name(then: &[Stmt], else_body: &[Stmt]) -> Option<String> {
    let t = first_direct_assign(then);
    let e = first_direct_assign(else_body);
    match (t, e) {
        (Some(a), Some(b)) if a == b => Some(a),
        _ => None,
    }
}

fn first_direct_assign(body: &[Stmt]) -> Option<String> {
    for stmt in body {
        if let Stmt::Assign { name, .. } = stmt {
            return Some(name.clone());
        }
    }
    None
}

pub fn operands_for_continue(
    params: &[String],
    edges: &[super::syntax::ContinueEdge],
) -> Option<Vec<Operand>> {
    let mut by_name: HashMap<&str, Operand> = HashMap::new();
    for e in edges {
        if by_name.contains_key(e.name.as_str()) {
            return None;
        }
        by_name.insert(&e.name, e.value.clone());
    }
    let mut ops = Vec::new();
    for p in params {
        ops.push(by_name.get(p.as_str())?.clone());
    }
    Some(ops)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body::syntax::{ContinueEdge, LoopParam, Operand, Rhs, Stmt, Type};

    fn name_operand(s: &str) -> Operand {
        Operand::Name(
            s.to_string(),
            Span {
                file: "t".into(),
                start: 0,
                end: 1,
            },
        )
    }

    #[test]
    fn direct_arm_sources_assign_and_loop_exit() {
        let body = vec![
            Stmt::Assign {
                name: "y".into(),
                rhs: Rhs::Hole {
                    span: Span {
                        file: "t".into(),
                        start: 0,
                        end: 1,
                    },
                    ty: Type::Named(
                        "i64".into(),
                        Span {
                            file: "t".into(),
                            start: 0,
                            end: 1,
                        },
                    ),
                },
                span: Span {
                    file: "t".into(),
                    start: 0,
                    end: 1,
                },
            },
            Stmt::Loop {
                span: Span {
                    file: "t".into(),
                    start: 0,
                    end: 1,
                },
                params: vec![LoopParam {
                    name: "x".into(),
                    init: name_operand("0"),
                    span: Span {
                        file: "t".into(),
                        start: 0,
                        end: 1,
                    },
                }],
                body: vec![],
            },
        ];
        let m = direct_arm_sources(&body);
        assert_eq!(m.get("y"), Some(&ArmSrc::Assign));
        assert_eq!(m.get("x"), Some(&ArmSrc::LoopExit));
    }

    #[test]
    fn assign_and_loop_on_same_name_is_absent() {
        let body = vec![
            Stmt::Assign {
                name: "x".into(),
                rhs: Rhs::Hole {
                    span: Span {
                        file: "t".into(),
                        start: 0,
                        end: 1,
                    },
                    ty: Type::Named(
                        "i64".into(),
                        Span {
                            file: "t".into(),
                            start: 0,
                            end: 1,
                        },
                    ),
                },
                span: Span {
                    file: "t".into(),
                    start: 0,
                    end: 1,
                },
            },
            Stmt::Loop {
                span: Span {
                    file: "t".into(),
                    start: 0,
                    end: 1,
                },
                params: vec![LoopParam {
                    name: "x".into(),
                    init: name_operand("0"),
                    span: Span {
                        file: "t".into(),
                        start: 0,
                        end: 1,
                    },
                }],
                body: vec![],
            },
        ];
        let m = direct_arm_sources(&body);
        assert!(!m.contains_key("x"));
    }

    #[test]
    fn publishes_loop_join_requires_loop_exit_on_one_arm() {
        let both_loop = NameSupply {
            name: "x".into(),
            then_arm: ArmSrc::LoopExit,
            else_arm: ArmSrc::LoopExit,
        };
        assert!(publishes_loop_join(&both_loop));
        let assign_only = NameSupply {
            name: "x".into(),
            then_arm: ArmSrc::Assign,
            else_arm: ArmSrc::Assign,
        };
        assert!(!publishes_loop_join(&assign_only));
    }

    #[test]
    fn if_join_bind_names_loop_join_and_s5() {
        let then = vec![Stmt::Assign {
            name: "x".into(),
            rhs: Rhs::Hole {
                span: Span {
                    file: "t".into(),
                    start: 0,
                    end: 1,
                },
                ty: Type::Named(
                    "i64".into(),
                    Span {
                        file: "t".into(),
                        start: 0,
                        end: 1,
                    },
                ),
            },
            span: Span {
                file: "t".into(),
                start: 0,
                end: 1,
            },
        }];
        let else_body = vec![Stmt::Loop {
            span: Span {
                file: "t".into(),
                start: 0,
                end: 1,
            },
            params: vec![LoopParam {
                name: "x".into(),
                init: name_operand("0"),
                span: Span {
                    file: "t".into(),
                    start: 0,
                    end: 1,
                },
            }],
            body: vec![],
        }];
        let site = JoinSite {
            id: SiteId(0),
            span: Span {
                file: "t".into(),
                start: 0,
                end: 1,
            },
            names: join_name_supplies(&then, &else_body),
        };
        let plan = CarryPlan::default();
        assert_eq!(
            if_join_bind_names(&plan, &site, &then, &else_body),
            vec!["x"]
        );
        let then2 = vec![
            Stmt::Assign {
                name: "x".into(),
                rhs: Rhs::Hole {
                    span: Span {
                        file: "t".into(),
                        start: 0,
                        end: 1,
                    },
                    ty: Type::Named(
                        "i64".into(),
                        Span {
                            file: "t".into(),
                            start: 0,
                            end: 1,
                        },
                    ),
                },
                span: Span {
                    file: "t".into(),
                    start: 0,
                    end: 1,
                },
            },
            Stmt::Assign {
                name: "y".into(),
                rhs: Rhs::Hole {
                    span: Span {
                        file: "t".into(),
                        start: 0,
                        end: 1,
                    },
                    ty: Type::Named(
                        "i64".into(),
                        Span {
                            file: "t".into(),
                            start: 0,
                            end: 1,
                        },
                    ),
                },
                span: Span {
                    file: "t".into(),
                    start: 0,
                    end: 1,
                },
            },
        ];
        let site2 = JoinSite {
            id: SiteId(1),
            span: Span {
                file: "t".into(),
                start: 0,
                end: 1,
            },
            names: join_name_supplies(&then2, &then2),
        };
        assert_eq!(if_join_bind_names(&plan, &site2, &then2, &then2), vec!["x"]);
    }

    #[test]
    fn operands_for_continue_order_follows_params() {
        let params = vec!["b".into(), "a".into()];
        let edges = vec![
            ContinueEdge {
                name: "a".into(),
                value: name_operand("a1"),
                span: Span {
                    file: "t".into(),
                    start: 0,
                    end: 1,
                },
            },
            ContinueEdge {
                name: "b".into(),
                value: name_operand("b1"),
                span: Span {
                    file: "t".into(),
                    start: 0,
                    end: 1,
                },
            },
        ];
        let ops = operands_for_continue(&params, &edges).expect("ops");
        assert_eq!(ops.len(), 2);
        assert!(matches!(&ops[0], Operand::Name(n, _) if n == "b1"));
        assert!(matches!(&ops[1], Operand::Name(n, _) if n == "a1"));
    }
}
