use clippy_utils::{diagnostics::span_lint_and_then, is_in_test};
use rustc_hir::{Arm, BinOpKind, Block, Expr, ExprKind, LoopSource, UnOp};
use rustc_lint::{LateContext, LateLintPass};
use rustc_session::{declare_lint, impl_lint_pass};
use rustc_span::{DesugaringKind, Span};

declare_lint! {
    /// ### What it does
    /// Detects `if`, `while`, and match guard predicates with too many
    /// short-circuit branches.
    ///
    /// ### Why is this bad?
    /// Wide boolean predicates are hard to read and test. Extracting named
    /// predicate helpers usually makes the conditional explain itself.
    pub CONDITIONAL_MAX_N_BRANCHES,
    Warn,
    "complex conditionals should be split when they exceed the branch limit"
}

#[derive(Copy, Clone)]
pub struct ConditionalMaxNBranches;

impl_lint_pass!(ConditionalMaxNBranches => [CONDITIONAL_MAX_N_BRANCHES]);

const MAX_BRANCHES: usize = 2;

impl<'tcx> LateLintPass<'tcx> for ConditionalMaxNBranches {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        if expr.span.from_expansion() || is_in_test(cx.tcx, expr.hir_id) {
            return;
        }

        match expr.kind {
            ExprKind::If(cond, ..) => {
                if expr.span.desugaring_kind() != Some(DesugaringKind::WhileLoop) {
                    inspect_condition(cx, ConditionKind::If, cond);
                }
            }
            ExprKind::Loop(block, _, LoopSource::While, _) => {
                if let Some(cond) = extract_while_condition(block) {
                    inspect_condition(cx, ConditionKind::While, cond);
                }
            }
            ExprKind::Match(_, arms, _) => inspect_match_guards(cx, arms),
            _ => {}
        }
    }
}

#[derive(Copy, Clone)]
enum ConditionKind {
    If,
    While,
    MatchGuard,
}

impl ConditionKind {
    const fn display_name(self) -> &'static str {
        match self {
            Self::If => "if condition",
            Self::While => "while condition",
            Self::MatchGuard => "match guard",
        }
    }
}

fn inspect_match_guards<'tcx>(cx: &LateContext<'tcx>, arms: &'tcx [Arm<'tcx>]) {
    for arm in arms {
        if let Some(guard) = arm.guard {
            inspect_condition(cx, ConditionKind::MatchGuard, guard);
        }
    }
}

fn inspect_condition<'tcx>(cx: &LateContext<'tcx>, kind: ConditionKind, expr: &'tcx Expr<'tcx>) {
    if expr.span.from_expansion() || matches!(expr.kind, ExprKind::Let(..)) {
        return;
    }

    let branches = count_branches(expr);
    if branches <= MAX_BRANCHES {
        return;
    }

    emit_diagnostic(cx, kind, expr.span, branches);
}

fn extract_while_condition<'tcx>(block: &'tcx Block<'tcx>) -> Option<&'tcx Expr<'tcx>> {
    let expr = block.expr?;
    if let ExprKind::If(cond, ..) = expr.kind {
        Some(cond)
    } else {
        None
    }
}

fn count_branches(expr: &Expr<'_>) -> usize {
    match expr.kind {
        ExprKind::Binary(op, lhs, rhs) if matches!(op.node, BinOpKind::And | BinOpKind::Or) => {
            count_branches(lhs) + count_branches(rhs)
        }
        ExprKind::Unary(UnOp::Not, inner) | ExprKind::DropTemps(inner) => count_branches(inner),
        ExprKind::Block(block, _) => block.expr.map_or(1, count_branches),
        ExprKind::If(cond, ..) => count_branches(cond),
        _ => 1,
    }
}

fn emit_diagnostic(cx: &LateContext<'_>, kind: ConditionKind, span: Span, branches: usize) {
    span_lint_and_then(
        cx,
        CONDITIONAL_MAX_N_BRANCHES,
        span,
        format!(
            "{} has {branches} boolean branches; keep conditionals to {MAX_BRANCHES} or fewer",
            kind.display_name(),
        ),
        |diag| {
            diag.help(
                "extract named predicate helpers or split the conditional into smaller checks",
            );
        },
    );
}
