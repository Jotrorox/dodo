//! A deliberately small language with an independent executable safety oracle.
//! No compiler AST, lowering, availability lattice, or checker verdict is used
//! to compute expectations. Branch choices are nondeterministic, as in Dodo's
//! conservative checks; loops enumerate concrete slot states until exhausted.
use dodoc::{parser, sema};
use sema::flow;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Default)]
struct Site {
    start: usize,
    end: usize,
}

#[derive(Debug)]
struct Local {
    name: &'static str,
    owned: bool,
    initialized: bool,
    declaration: usize,
}

#[derive(Clone, Copy, Debug)]
enum Action {
    Initialize,
    Read,
    Move,
}

#[derive(Clone, Copy, Debug)]
enum Loop {
    Conditional,
    Range,
    Forever,
}

#[derive(Debug)]
enum Stmt {
    Action(Action, usize, Site),
    Branch(Vec<Stmt>, Vec<Stmt>),
    Scope(usize, Vec<Stmt>),
    Loop(Loop, Vec<Stmt>),
    Break,
    Continue,
    Return,
}

fn action(kind: Action, local: usize) -> Stmt {
    Stmt::Action(kind, local, Site::default())
}

/// Byte decoding always terminates, including on empty or adversarial inputs.
/// The node/depth limits also bound the oracle's finite state space.
struct Generator<'a> {
    bytes: &'a [u8],
    offset: usize,
    budget: usize,
    locals: Vec<Local>,
}

impl Generator<'_> {
    fn byte(&mut self) -> u8 {
        let value = self.bytes.get(self.offset).copied().unwrap_or(0);
        self.offset += 1;
        value
    }

    fn block(&mut self, visible: [usize; 2], depth: usize) -> Vec<Stmt> {
        let count = 1 + self.byte() % 3;
        let mut block = Vec::new();
        for _ in 0..count {
            if self.budget == 0 {
                break;
            }
            self.budget -= 1;
            let choice = self.byte() % if depth < 2 { 9 } else { 5 };
            block.push(match choice {
                0 => action(Action::Initialize, visible[0]),
                1 => action(Action::Initialize, visible[1]),
                2 => action(Action::Read, visible[0]),
                3 => action(Action::Read, visible[1]),
                4 => action(Action::Move, visible[1]),
                5 => Stmt::Branch(
                    self.block(visible, depth + 1),
                    self.block(visible, depth + 1),
                ),
                6 => {
                    let index = usize::from(self.byte() % 2);
                    let outer = &self.locals[visible[index]];
                    let local = Local {
                        name: outer.name,
                        owned: outer.owned,
                        initialized: self.byte().is_multiple_of(2),
                        declaration: 0,
                    };
                    let id = self.locals.len();
                    self.locals.push(local);
                    let mut inner = visible;
                    inner[index] = id;
                    Stmt::Scope(id, self.block(inner, depth + 1))
                }
                7 => {
                    let form = self.byte() % 4;
                    // Probe owned storage before mutation on repeating loops.
                    // This common subset makes a lost value on a back edge an actual
                    // bad read, not merely an AST loop-invariant restriction.
                    let mut body = if form == 2 {
                        vec![] // One iteration: only a break leaves this loop.
                    } else {
                        vec![action(Action::Read, visible[1])]
                    };
                    body.extend(self.block(visible, depth + 1));
                    let kind = match form {
                        0 => Loop::Conditional,
                        1 => Loop::Range,
                        2 => {
                            body.push(Stmt::Break);
                            Loop::Forever
                        }
                        _ => {
                            body.push(Stmt::Branch(vec![Stmt::Continue], vec![Stmt::Break]));
                            Loop::Forever
                        }
                    };
                    Stmt::Loop(kind, body)
                }
                _ => {
                    let mut early = self.block(visible, depth + 1);
                    early.push(Stmt::Return);
                    Stmt::Branch(early, self.block(visible, depth + 1))
                }
            });
        }
        block
    }
}

#[derive(Debug)]
struct Case {
    locals: Vec<Local>,
    body: Vec<Stmt>,
}

impl Case {
    fn decode(bytes: &[u8]) -> Self {
        let mut generator = Generator {
            bytes,
            offset: 0,
            budget: 20,
            locals: Vec::new(),
        };
        for (name, owned) in [("x", false), ("s", true)] {
            let initialized = generator.byte().is_multiple_of(2);
            generator.locals.push(Local {
                name,
                owned,
                initialized,
                declaration: 0,
            });
        }
        let mut body = generator.block([0, 1], 0);
        body.extend([action(Action::Read, 0), action(Action::Read, 1)]);
        Self {
            locals: generator.locals,
            body,
        }
    }

    fn render(&mut self) -> String {
        fn declare(id: usize, locals: &mut [Local], source: &mut String) {
            let local = &mut locals[id];
            local.declaration = source.len();
            source.push_str(if local.owned { "S " } else { "u8 " });
            source.push_str(local.name);
            if local.initialized {
                source.push_str(if local.owned { " = S{n: 1}" } else { " = 1" });
            }
            source.push('\n');
        }
        fn block(body: &mut [Stmt], locals: &mut [Local], source: &mut String) {
            for stmt in body {
                match stmt {
                    Stmt::Action(kind, id, site) => {
                        let local = &locals[*id];
                        match kind {
                            Action::Initialize => {
                                source.push_str(local.name);
                                source.push_str(if local.owned {
                                    " = S{n: 2}\n"
                                } else {
                                    " = 2\n"
                                });
                            }
                            Action::Read | Action::Move => {
                                source.push_str(if matches!(kind, Action::Move) {
                                    "take("
                                } else {
                                    "_ = "
                                });
                                site.start = source.len();
                                source.push_str(local.name);
                                if matches!(kind, Action::Read) && local.owned {
                                    source.push_str(".n");
                                }
                                site.end = source.len();
                                if matches!(kind, Action::Move) {
                                    source.push(')');
                                }
                                source.push('\n');
                            }
                        }
                    }
                    Stmt::Branch(yes, no) => {
                        source.push_str("if b {\n");
                        block(yes, locals, source);
                        source.push_str("} else {\n");
                        block(no, locals, source);
                        source.push_str("}\n");
                    }
                    Stmt::Scope(id, inner) => {
                        source.push_str("{\n");
                        declare(*id, locals, source);
                        block(inner, locals, source);
                        source.push_str("}\n");
                    }
                    Stmt::Loop(kind, inner) => {
                        source.push_str(match kind {
                            Loop::Conditional => "for b {\n",
                            Loop::Range => "for _ in 0..2 {\n",
                            Loop::Forever => "for {\n",
                        });
                        block(inner, locals, source);
                        source.push_str("}\n");
                    }
                    Stmt::Break => source.push_str("break\n"),
                    Stmt::Continue => source.push_str("continue\n"),
                    Stmt::Return => source.push_str("return\n"),
                }
            }
        }
        let mut source = String::from(
            "package generated\nstruct S { u8 n }\nfn take(s: S) {}\nfn f(b: bool) {\n",
        );
        declare(0, &mut self.locals, &mut source);
        declare(1, &mut self.locals, &mut source);
        block(&mut self.body, &mut self.locals, &mut source);
        source.push_str("}\n");
        source
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Slot {
    Uninitialized,
    Live,
    Moved,
}
type States = BTreeSet<Vec<Slot>>;
// Use and declaration positions distinguish shadowed names and repeated uses.
type IssueKey = (usize, usize, usize);
type Issues = BTreeMap<IssueKey, bool>;

#[derive(Default)]
struct Paths {
    next: States,
    breaks: States,
    continues: States,
}

struct Oracle<'a> {
    locals: &'a [Local],
    issues: Issues,
}

impl Oracle<'_> {
    fn block(&mut self, body: &[Stmt], mut states: States) -> Paths {
        let mut paths = Paths::default();
        for stmt in body {
            let exits = self.statement(stmt, states);
            paths.breaks.extend(exits.breaks);
            paths.continues.extend(exits.continues);
            states = exits.next;
        }
        paths.next = states;
        paths
    }

    fn statement(&mut self, stmt: &Stmt, states: States) -> Paths {
        let mut paths = Paths::default();
        match stmt {
            Stmt::Action(kind, id, site) => {
                for mut state in states {
                    if !matches!(kind, Action::Initialize) && state[*id] != Slot::Live {
                        let key = (site.start, site.end, self.locals[*id].declaration);
                        let moved = self.issues.entry(key).or_default();
                        *moved |= state[*id] == Slot::Moved;
                    }
                    match kind {
                        Action::Initialize => state[*id] = Slot::Live,
                        Action::Move => state[*id] = Slot::Moved,
                        Action::Read => (),
                    }
                    paths.next.insert(state);
                }
            }
            Stmt::Branch(yes, no) => {
                let first = self.block(yes, states.clone());
                let second = self.block(no, states);
                paths.next.extend(first.next);
                paths.next.extend(second.next);
                paths.breaks.extend(first.breaks);
                paths.breaks.extend(second.breaks);
                paths.continues.extend(first.continues);
                paths.continues.extend(second.continues);
            }
            Stmt::Scope(id, body) => {
                let entered = states
                    .into_iter()
                    .map(|mut state| {
                        state[*id] = if self.locals[*id].initialized {
                            Slot::Live
                        } else {
                            Slot::Uninitialized
                        };
                        state
                    })
                    .collect();
                paths = self.block(body, entered);
                // Scope cleanup applies on fallthrough, break, and continue.
                for exits in [&mut paths.next, &mut paths.breaks, &mut paths.continues] {
                    *exits = std::mem::take(exits)
                        .into_iter()
                        .map(|mut state| {
                            state[*id] = Slot::Uninitialized;
                            state
                        })
                        .collect();
                }
            }
            Stmt::Loop(kind, body) => {
                let mut pending: Vec<_> = states.into_iter().collect();
                let mut visited = States::new();
                while let Some(state) = pending.pop() {
                    if !visited.insert(state.clone()) {
                        continue;
                    }
                    if !matches!(kind, Loop::Forever) {
                        // Conditional and range loops can execute zero times.
                        paths.next.insert(state.clone());
                    }
                    let iteration = self.block(body, BTreeSet::from([state]));
                    paths.next.extend(iteration.breaks);
                    pending.extend(iteration.next);
                    pending.extend(iteration.continues);
                    // Return paths leave the function and never feed a loop.
                }
            }
            Stmt::Break => paths.breaks = states,
            Stmt::Continue => paths.continues = states,
            Stmt::Return => (),
        }
        paths
    }
}

fn expectations(case: &Case) -> Issues {
    let mut state = vec![Slot::Uninitialized; case.locals.len()];
    for (id, local) in case.locals.iter().take(2).enumerate() {
        if local.initialized {
            state[id] = Slot::Live;
        }
    }
    let mut oracle = Oracle {
        locals: &case.locals,
        issues: Issues::new(),
    };
    let exits = oracle.block(&case.body, BTreeSet::from([state]));
    assert!(exits.breaks.is_empty() && exits.continues.is_empty());
    oracle.issues
}

fn verify_case(case: &mut Case, pointer_bits: u32) -> bool {
    let source = case.render();
    let expected = expectations(case);
    let program = parser::parse(&source).unwrap_or_else(|error| panic!("{error:?}\n{source}"));
    let comparison = flow::compare_checkers(&program, pointer_bits);
    for (name, report) in [
        ("AST", &comparison.ast),
        ("flow", &comparison.flow),
        ("combined", &comparison.combined),
    ] {
        assert_eq!(
            report.diagnostics.is_empty(),
            expected.is_empty(),
            "{name}, {pointer_bits}-bit: {report:?}\nexpected {expected:?}\n{source}"
        );
        for diagnostic in &report.diagnostics {
            assert!(
                diagnostic.message.contains("uninitialized")
                    || diagnostic.message.contains("moved"),
                "unrelated {name} rejection: {diagnostic:?}\n{source}"
            );
        }
        if let Some(first) = report.diagnostics.first() {
            assert!(
                case.locals.iter().any(|local| {
                    expected.keys().any(|key| key.2 == local.declaration)
                        && first.message.contains(&format!("`{}`", local.name))
                }),
                "{name} rejected the wrong binding: {first:?}\n{source}"
            );
        }
    }
    assert!(comparison.ast.coverage.is_none());
    // Inspect both reports against the oracle. A skip or missing coverage must
    // never count as success, even when no user-facing diagnostic was emitted.
    for report in [&comparison.flow, &comparison.combined] {
        let coverage = report.coverage.as_ref().expect("prepared flow coverage");
        assert_eq!(coverage.bodies.len(), 2, "{source}");
        for name in ["f", "take"] {
            assert!(
                coverage.bodies.iter().any(|body| body.name == name),
                "missing {name}: {source}"
            );
        }
        for body in &coverage.bodies {
            let issues = match &body.status {
                flow::BodyStatus::Analyzed => &[][..],
                flow::BodyStatus::Diagnostics(issues) => issues.as_slice(),
                other => panic!("unexpected fallback for {}: {other:?}\n{source}", body.name),
            };
            let actual: Issues = issues
                .iter()
                .map(|issue| {
                    let local = case
                        .locals
                        .iter()
                        .find(|local| local.declaration == issue.declaration.start)
                        .unwrap_or_else(|| panic!("unknown declaration: {issue:?}\n{source}"));
                    assert_eq!(issue.name, local.name, "{source}");
                    (
                        (issue.span.start, issue.span.end, issue.declaration.start),
                        issue.moved_at.is_some(),
                    )
                })
                .collect();
            assert_eq!(actual.len(), issues.len(), "duplicate findings: {source}");
            let wanted = if body.name == "f" {
                &expected
            } else {
                &Issues::new()
            };
            assert_eq!(
                &actual, wanted,
                "{}: {pointer_bits}-bit\n{source}",
                body.name
            );
        }
    }
    assert_eq!(
        sema::check_for_target(&mut program.clone(), pointer_bits).is_ok(),
        expected.is_empty(),
        "fail-fast production: {pointer_bits}-bit\n{source}"
    );
    expected.is_empty()
}

/// Shared by the deterministic integration tests and the libFuzzer target.
/// Panics include the complete generated source; libFuzzer retains input bytes.
pub fn verify(bytes: &[u8], pointer_bits: u32) -> bool {
    verify_case(&mut Case::decode(bytes), pointer_bits)
}

/// Stable, fully specified PRNG; a seed reproduces exactly the same byte stream
/// on every host and toolchain, without a random dependency or wall-clock seed.
#[cfg(test)]
pub fn seeded_bytes(mut seed: u64) -> [u8; 128] {
    let mut bytes = [0; 128];
    for byte in &mut bytes {
        seed = seed.wrapping_add(0x9e3779b97f4a7c15);
        let mut mixed = seed;
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94d049bb133111eb);
        *byte = (mixed ^ (mixed >> 31)) as u8;
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn case(body: Vec<Stmt>, initialized: [bool; 2]) -> Case {
        Case {
            locals: [("x", false), ("s", true)]
                .into_iter()
                .zip(initialized)
                .map(|((name, owned), initialized)| Local {
                    name,
                    owned,
                    initialized,
                    declaration: 0,
                })
                .collect(),
            body,
        }
    }

    fn assert_outcome(mut case: Case, expected: &[(usize, bool)]) {
        let source = case.render();
        let actual: Vec<_> = expectations(&case)
            .iter()
            .map(|(key, moved)| {
                (
                    case.locals
                        .iter()
                        .position(|local| local.declaration == key.2)
                        .unwrap(),
                    *moved,
                )
            })
            .collect();
        assert_eq!(actual, expected, "oracle: {source}");
        for pointer_bits in [32, 64] {
            verify_case(&mut case, pointer_bits);
        }
    }

    #[test]
    fn oracle_branch_and_return_outcomes_are_specified_independently() {
        for initialize_else in [false, true] {
            assert_outcome(
                case(
                    vec![
                        Stmt::Branch(
                            vec![action(Action::Initialize, 0)],
                            if initialize_else {
                                vec![action(Action::Initialize, 0)]
                            } else {
                                vec![]
                            },
                        ),
                        action(Action::Read, 0),
                    ],
                    [false, true],
                ),
                if initialize_else { &[] } else { &[(0, false)] },
            );
        }
        assert_outcome(
            case(
                vec![
                    Stmt::Branch(
                        vec![action(Action::Move, 1), Stmt::Return],
                        vec![action(Action::Initialize, 0)],
                    ),
                    action(Action::Read, 0),
                    action(Action::Read, 1),
                ],
                [false, true],
            ),
            &[],
        );
    }

    #[test]
    fn oracle_loop_back_edges_reinitialization_and_exits_are_specified() {
        for kind in [Loop::Conditional, Loop::Range, Loop::Forever] {
            for restore in [false, true] {
                let mut body = vec![action(Action::Move, 1)];
                if restore {
                    body.push(action(Action::Initialize, 1));
                }
                body.push(Stmt::Branch(vec![Stmt::Continue], vec![Stmt::Break]));
                assert_outcome(
                    case(vec![Stmt::Loop(kind, body)], [true, true]),
                    if restore { &[] } else { &[(1, true)] },
                );
            }
            assert_outcome(
                case(
                    vec![
                        Stmt::Loop(kind, vec![action(Action::Move, 1), Stmt::Break]),
                        action(Action::Read, 1),
                    ],
                    [true, true],
                ),
                &[(1, true)],
            );
        }
        assert_outcome(
            case(
                vec![Stmt::Loop(
                    Loop::Conditional,
                    vec![Stmt::Loop(
                        Loop::Forever,
                        vec![action(Action::Move, 1), Stmt::Break],
                    )],
                )],
                [true, true],
            ),
            &[(1, true)],
        );
    }

    #[test]
    fn oracle_loop_initialization_requires_every_reachable_exit() {
        for kind in [Loop::Conditional, Loop::Range, Loop::Forever] {
            assert_outcome(
                case(
                    vec![
                        Stmt::Loop(kind, vec![action(Action::Initialize, 0), Stmt::Break]),
                        action(Action::Read, 0),
                    ],
                    [false, true],
                ),
                if matches!(kind, Loop::Forever) {
                    &[]
                } else {
                    &[(0, false)]
                },
            );
        }
        for skip_assignment in [false, true] {
            assert_outcome(
                case(
                    vec![
                        Stmt::Loop(
                            Loop::Forever,
                            vec![Stmt::Branch(
                                vec![if skip_assignment {
                                    Stmt::Break
                                } else {
                                    Stmt::Continue
                                }],
                                vec![action(Action::Initialize, 0), Stmt::Break],
                            )],
                        ),
                        action(Action::Read, 0),
                    ],
                    [false, true],
                ),
                if skip_assignment { &[(0, false)] } else { &[] },
            );
        }
    }

    #[test]
    fn oracle_shadowing_preserves_outer_identity_and_refreshes_loop_locals() {
        let mut shadowed = case(
            vec![
                Stmt::Scope(2, vec![action(Action::Initialize, 2)]),
                action(Action::Read, 0),
            ],
            [false, true],
        );
        shadowed.locals.push(Local {
            name: "x",
            owned: false,
            initialized: false,
            declaration: 0,
        });
        assert_outcome(shadowed, &[(0, false)]);

        let mut refreshed = case(
            vec![
                Stmt::Loop(
                    Loop::Conditional,
                    vec![Stmt::Scope(
                        2,
                        vec![action(Action::Move, 2), Stmt::Continue],
                    )],
                ),
                action(Action::Read, 1),
            ],
            [true, true],
        );
        refreshed.locals.push(Local {
            name: "s",
            owned: true,
            initialized: true,
            declaration: 0,
        });
        assert_outcome(refreshed, &[]);
    }
}
