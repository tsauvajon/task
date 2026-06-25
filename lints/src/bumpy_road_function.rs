use std::ops::RangeInclusive;

use clippy_utils::{diagnostics::span_lint_and_then, is_in_test};
use rustc_hir::{
    self as hir, BinOpKind, Body, Expr, ExprKind, FnDecl, LoopSource, UnOp, intravisit,
};
use rustc_lint::{LateContext, LateLintPass};
use rustc_session::{declare_lint, impl_lint_pass};
use rustc_span::{DesugaringKind, Span, Symbol};

declare_lint! {
    /// ### What it does
    /// Detects functions with two or more separated clusters of nested
    /// conditional complexity.
    ///
    /// ### Why is this bad?
    /// Multiple complexity clusters in one function usually indicate separate
    /// decisions that should be extracted into named helpers.
    pub BUMPY_ROAD_FUNCTION,
    Warn,
    "functions should avoid multiple separated clusters of conditional complexity"
}

#[derive(Copy, Clone)]
pub struct BumpyRoadFunction;

impl_lint_pass!(BumpyRoadFunction => [BUMPY_ROAD_FUNCTION]);

const THRESHOLD: f64 = 2.5;
const WINDOW: usize = 3;
const MIN_BUMP_LINES: usize = 2;

impl<'tcx> LateLintPass<'tcx> for BumpyRoadFunction {
    fn check_fn(
        &mut self,
        cx: &LateContext<'tcx>,
        kind: intravisit::FnKind<'tcx>,
        _decl: &'tcx FnDecl<'tcx>,
        body: &'tcx Body<'tcx>,
        span: Span,
        local_def_id: hir::def_id::LocalDefId,
    ) {
        if span.from_expansion() {
            return;
        }

        let hir_id = cx.tcx.local_def_id_to_hir_id(local_def_id);
        if is_in_test(cx.tcx, hir_id) {
            return;
        }

        let name = match kind {
            intravisit::FnKind::ItemFn(ident, ..) | intravisit::FnKind::Method(ident, ..) => {
                ident.name
            }
            intravisit::FnKind::Closure => return,
        };

        analyse_body(cx, name, span, body.value);
    }
}

#[derive(Clone, Copy)]
struct Segment {
    start_line: usize,
    end_line: usize,
    value: f64,
}

#[derive(Clone, Copy)]
struct Bump {
    start_index: usize,
    end_index: usize,
    area: f64,
}

fn analyse_body<'tcx>(
    cx: &LateContext<'tcx>,
    name: Symbol,
    primary_span: Span,
    body: &'tcx Expr<'tcx>,
) {
    let Some(function_lines) = span_line_range(cx, body.span) else {
        return;
    };

    let mut segments = Vec::new();
    let mut builder = SegmentBuilder::new(cx, function_lines.clone(), &mut segments);
    builder.visit_expr(body);

    let signal = rasterize_signal(&function_lines, &segments);
    let smoothed = smooth_moving_average(&signal, WINDOW);
    let bumps = detect_bumps(&smoothed, THRESHOLD, MIN_BUMP_LINES);
    if bumps.len() < 2 {
        return;
    }

    let mut ranked = bumps;
    ranked.sort_by(|a, b| b.area.total_cmp(&a.area));
    let first = ranked[0];
    let second = ranked[1];

    span_lint_and_then(
        cx,
        BUMPY_ROAD_FUNCTION,
        primary_span,
        format!(
            "`{name}` has {} separated clusters of conditional complexity",
            ranked.len(),
        ),
        |diag| {
            diag.help("extract the largest conditional clusters into named helper functions");
            diag.note(format!(
                "largest clusters cover lines {}-{} and {}-{}",
                function_lines.start() + first.start_index,
                function_lines.start() + first.end_index,
                function_lines.start() + second.start_index,
                function_lines.start() + second.end_index,
            ));
        },
    );
}

struct SegmentBuilder<'cx, 'tcx> {
    cx: &'cx LateContext<'tcx>,
    function_lines: RangeInclusive<usize>,
    segments: &'cx mut Vec<Segment>,
}

impl<'cx, 'tcx> SegmentBuilder<'cx, 'tcx> {
    fn new(
        cx: &'cx LateContext<'tcx>,
        function_lines: RangeInclusive<usize>,
        segments: &'cx mut Vec<Segment>,
    ) -> Self {
        Self {
            cx,
            function_lines,
            segments,
        }
    }

    fn visit_expr(&mut self, expr: &'tcx Expr<'tcx>) {
        if expr.span.from_expansion() {
            return;
        }

        match expr.kind {
            ExprKind::If(cond, then_expr, else_expr) => {
                if expr.span.desugaring_kind() == Some(DesugaringKind::WhileLoop) {
                    intravisit::walk_expr(self, expr);
                    return;
                }
                self.push_predicate_segment(cond);
                self.visit_expr_with_depth(then_expr);
                if let Some(else_expr) = else_expr {
                    self.visit_expr_with_depth(else_expr);
                }
            }
            ExprKind::Loop(block, _, source, _) => {
                if source == LoopSource::While
                    && let Some((cond, body_expr)) = extract_while_components(block)
                {
                    self.push_predicate_segment(cond);
                    self.visit_expr_with_depth(body_expr);
                } else {
                    self.push_depth_segment(block.span);
                    self.visit_block(block);
                }
            }
            ExprKind::Match(scrutinee, arms, _) => {
                self.visit_expr(scrutinee);
                for arm in arms {
                    if let Some(guard) = arm.guard {
                        self.push_predicate_segment(guard);
                    }
                    self.visit_expr(arm.body);
                }
            }
            ExprKind::Block(block, _) => self.visit_block(block),
            ExprKind::Closure(..) => {}
            _ => intravisit::walk_expr(self, expr),
        }
    }

    fn visit_block(&mut self, block: &'tcx hir::Block<'tcx>) {
        for stmt in block.stmts {
            intravisit::walk_stmt(self, stmt);
        }
        if let Some(expr) = block.expr {
            self.visit_expr(expr);
        }
    }

    fn visit_expr_with_depth(&mut self, expr: &'tcx Expr<'tcx>) {
        self.push_depth_segment(expr.span);
        self.visit_expr(expr);
    }

    fn push_depth_segment(&mut self, span: Span) {
        self.push_segment(span, 1.0);
    }

    fn push_predicate_segment(&mut self, expr: &'tcx Expr<'tcx>) {
        if matches!(expr.kind, ExprKind::Let(..)) {
            return;
        }
        self.push_segment(expr.span, count_branches(expr) as f64 * 0.5);
    }

    fn push_segment(&mut self, span: Span, value: f64) {
        if span.from_expansion() {
            return;
        }

        let Some(lines) = span_line_range(self.cx, span) else {
            return;
        };
        if lines.end() < self.function_lines.start() || lines.start() > self.function_lines.end() {
            return;
        }

        self.segments.push(Segment {
            start_line: *lines.start(),
            end_line: *lines.end(),
            value,
        });
    }
}

impl<'tcx> intravisit::Visitor<'tcx> for SegmentBuilder<'_, 'tcx> {
    fn visit_expr(&mut self, expr: &'tcx Expr<'tcx>) {
        Self::visit_expr(self, expr);
    }

    fn visit_block(&mut self, block: &'tcx hir::Block<'tcx>) {
        Self::visit_block(self, block);
    }
}

fn extract_while_components<'tcx>(
    block: &'tcx hir::Block<'tcx>,
) -> Option<(&'tcx Expr<'tcx>, &'tcx Expr<'tcx>)> {
    let expr = block.expr?;
    if let ExprKind::If(cond, then_expr, ..) = expr.kind {
        Some((cond, then_expr))
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

fn span_line_range(cx: &LateContext<'_>, span: Span) -> Option<RangeInclusive<usize>> {
    let info = cx.tcx.sess.source_map().span_to_lines(span).ok()?;
    let first = info.lines.first()?;
    let last = info.lines.last()?;
    Some((first.line_index + 1)..=(last.line_index + 1))
}

fn rasterize_signal(function_lines: &RangeInclusive<usize>, segments: &[Segment]) -> Vec<f64> {
    let len = function_lines.end() - function_lines.start() + 1;
    let mut signal = vec![0.0; len];
    for segment in segments {
        let start = segment.start_line.max(*function_lines.start()) - function_lines.start();
        let end = segment.end_line.min(*function_lines.end()) - function_lines.start();
        for value in &mut signal[start..=end] {
            *value += segment.value;
        }
    }
    signal
}

fn smooth_moving_average(signal: &[f64], window: usize) -> Vec<f64> {
    if signal.is_empty() || window <= 1 {
        return signal.to_vec();
    }

    let radius = window / 2;
    (0..signal.len())
        .map(|index| {
            let start = index.saturating_sub(radius);
            let end = (index + radius).min(signal.len() - 1);
            let values = &signal[start..=end];
            values.iter().sum::<f64>() / values.len() as f64
        })
        .collect()
}

fn detect_bumps(signal: &[f64], threshold: f64, min_bump_lines: usize) -> Vec<Bump> {
    let mut bumps = Vec::new();
    let mut current_start = None;
    let mut area = 0.0;

    for (index, value) in signal.iter().copied().enumerate() {
        if value >= threshold {
            current_start.get_or_insert(index);
            area += value - threshold;
            continue;
        }

        if let Some(start) = current_start.take() {
            push_bump(
                &mut bumps,
                start,
                index.saturating_sub(1),
                area,
                min_bump_lines,
            );
            area = 0.0;
        }
    }

    if let Some(start) = current_start {
        push_bump(&mut bumps, start, signal.len() - 1, area, min_bump_lines);
    }

    bumps
}

fn push_bump(
    bumps: &mut Vec<Bump>,
    start_index: usize,
    end_index: usize,
    area: f64,
    min_bump_lines: usize,
) {
    if end_index + 1 - start_index >= min_bump_lines {
        bumps.push(Bump {
            start_index,
            end_index,
            area,
        });
    }
}
