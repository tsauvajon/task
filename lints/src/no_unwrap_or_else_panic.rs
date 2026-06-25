use clippy_utils::{diagnostics::span_lint_and_then, is_in_test};
use rustc_hir::{self as hir, Expr, ExprKind, intravisit};
use rustc_lint::{LateContext, LateLintPass};
use rustc_middle::ty;
use rustc_session::{declare_lint, impl_lint_pass};
use rustc_span::sym;

declare_lint! {
    /// ### What it does
    /// Detects `Option::unwrap_or_else` and `Result::unwrap_or_else` fallbacks
    /// whose closure panics directly, or panics indirectly through `unwrap` or
    /// `expect`.
    ///
    /// ### Why is this bad?
    /// A panicking fallback hides the fact that the code is still an unwrap. Use
    /// `expect`, propagate the error, or provide a real fallback instead.
    pub NO_UNWRAP_OR_ELSE_PANIC,
    Warn,
    "unwrap_or_else fallback closures should not panic"
}

#[derive(Copy, Clone)]
pub struct NoUnwrapOrElsePanic;

impl_lint_pass!(NoUnwrapOrElsePanic => [NO_UNWRAP_OR_ELSE_PANIC]);

impl<'tcx> LateLintPass<'tcx> for NoUnwrapOrElsePanic {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        if expr.span.from_expansion() || is_in_test(cx.tcx, expr.hir_id) {
            return;
        }

        let ExprKind::MethodCall(segment, receiver, args, _) = expr.kind else {
            return;
        };

        if segment.ident.name.as_str() != "unwrap_or_else"
            || !receiver_is_option_or_result(cx, receiver)
        {
            return;
        }

        let Some(fallback) = args.first() else {
            return;
        };
        let Some(body_id) = closure_body(fallback) else {
            return;
        };

        if !closure_panics(cx, body_id) {
            return;
        }

        span_lint_and_then(
            cx,
            NO_UNWRAP_OR_ELSE_PANIC,
            expr.span,
            "unwrap_or_else fallback closure can panic",
            |diag| {
                diag.span_note(receiver.span, "receiver is an Option or Result");
                diag.help("propagate the error, use expect with context, or return a non-panicking fallback");
            },
        );
    }
}

fn closure_body(expr: &Expr<'_>) -> Option<hir::BodyId> {
    match expr.kind {
        ExprKind::Closure(hir::Closure { body, .. }) => Some(*body),
        _ => None,
    }
}

fn closure_panics<'tcx>(cx: &LateContext<'tcx>, body_id: hir::BodyId) -> bool {
    let mut detector = PanicDetector { cx, panics: false };
    let body = cx.tcx.hir_body(body_id);
    intravisit::Visitor::visit_body(&mut detector, body);
    detector.panics
}

struct PanicDetector<'cx, 'tcx> {
    cx: &'cx LateContext<'tcx>,
    panics: bool,
}

impl<'tcx> intravisit::Visitor<'tcx> for PanicDetector<'_, 'tcx> {
    fn visit_expr(&mut self, expr: &'tcx Expr<'tcx>) {
        if self.panics {
            return;
        }

        if is_panic_call(self.cx, expr) || is_unwrap_or_expect(self.cx, expr) {
            self.panics = true;
            return;
        }

        intravisit::walk_expr(self, expr);
    }
}

fn is_unwrap_or_expect<'tcx>(cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) -> bool {
    let ExprKind::MethodCall(segment, receiver, ..) = expr.kind else {
        return false;
    };

    matches!(segment.ident.name.as_str(), "unwrap" | "expect")
        && receiver_is_option_or_result(cx, receiver)
}

fn is_panic_call(cx: &LateContext<'_>, expr: &Expr<'_>) -> bool {
    let ExprKind::Call(callee, _) = expr.kind else {
        return false;
    };

    let Some(def_id) = cx
        .typeck_results()
        .type_dependent_def_id(callee.hir_id)
        .or_else(|| match callee.kind {
            ExprKind::Path(qpath) => cx.qpath_res(&qpath, callee.hir_id).opt_def_id(),
            _ => None,
        })
    else {
        return false;
    };

    let path = cx.tcx.def_path_str(def_id);
    path.contains("::panicking::panic")
        || path.contains("::panic::panic_any")
        || path.contains("::rt::panic")
        || path.contains("::rt::begin_panic")
}

fn receiver_is_option_or_result<'tcx>(cx: &LateContext<'tcx>, receiver: &'tcx Expr<'tcx>) -> bool {
    let ty = cx.typeck_results().expr_ty(receiver).peel_refs();
    matches!(ty.kind(), ty::Adt(adt, _) if cx.tcx.is_diagnostic_item(sym::Option, adt.did()) || cx.tcx.is_diagnostic_item(sym::Result, adt.did()))
}
