use clippy_utils::{diagnostics::span_lint_and_then, source::snippet_opt};
use rustc_hir::{Expr, ExprKind};
use rustc_lint::{LateContext, LateLintPass};
use rustc_middle::ty;
use rustc_session::{declare_lint, impl_lint_pass};

declare_lint! {
    /// ### What it does
    /// Detects `.into()` conversions.
    ///
    /// ### Why is this bad?
    /// `.into()` hides the conversion target behind type inference. Writing
    /// `Target::from(value)` keeps the target type visible where the conversion
    /// happens.
    pub EXPLICIT_FROM,
    Warn,
    "use explicit From conversions instead of into calls"
}

#[derive(Copy, Clone)]
pub struct ExplicitFrom;

impl_lint_pass!(ExplicitFrom => [EXPLICIT_FROM]);

impl<'tcx> LateLintPass<'tcx> for ExplicitFrom {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        if expr.span.from_expansion() {
            return;
        }

        let ExprKind::MethodCall(segment, receiver, args, _) = expr.kind else {
            return;
        };

        if segment.ident.name.as_str() != "into" || !args.is_empty() {
            return;
        }

        let receiver_ty = cx.typeck_results().expr_ty(receiver).peel_refs();
        if matches!(receiver_ty.kind(), ty::Param(_)) {
            return;
        }

        let target_ty = cx.typeck_results().expr_ty(expr);
        let receiver_snippet = snippet_opt(cx, receiver.span).unwrap_or_else(|| "value".to_owned());

        span_lint_and_then(
            cx,
            EXPLICIT_FROM,
            expr.span,
            ".into() hides the conversion target type",
            |diag| {
                diag.help(format!(
                    "write the target explicitly, for example `{target_ty}::from({receiver_snippet})`"
                ));
            },
        );
    }
}
