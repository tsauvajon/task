use clippy_utils::{diagnostics::span_lint_and_then, is_in_test, is_lang_item_or_ctor};
use rustc_ast::LitKind;
use rustc_hir::{
    self as hir, Expr, ExprKind, ImplItem, ImplItemImplKind, ImplItemKind, LangItem, Pat,
    PatExprKind, PatKind, def::DefKind, def_id::DefId,
};
use rustc_lint::{LateContext, LateLintPass};
use rustc_middle::ty::{self, Ty};
use rustc_session::{declare_lint, impl_lint_pass};
use rustc_span::Span;

declare_lint! {
    /// ### What it does
    /// Detects manual enum-to-string and string-to-enum literal mappings.
    ///
    /// ### Why is this bad?
    /// Plain enum string conversions are easier to keep canonical when they are
    /// expressed through `strum` derives and per-variant serialization overrides.
    ///
    /// This lint is intentionally conservative. It may miss manual conversions
    /// that use fallback arms, `write!` macro formatting, or transformed parse
    /// inputs such as `s.to_lowercase().as_str()`.
    pub MANUAL_ENUM_STRING_CONVERSION,
    Warn,
    "manual enum string conversion can be represented with strum derives"
}

#[derive(Copy, Clone)]
pub struct ManualEnumStringConversion;

impl_lint_pass!(ManualEnumStringConversion => [MANUAL_ENUM_STRING_CONVERSION]);

impl<'tcx> LateLintPass<'tcx> for ManualEnumStringConversion {
    fn check_impl_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx ImplItem<'tcx>) {
        if item.span.from_expansion() || is_in_test(cx.tcx, item.hir_id()) {
            return;
        }

        let ImplItemKind::Fn(sig, body_id) = &item.kind else {
            return;
        };
        let Some(impl_def_id) = cx.tcx.impl_of_assoc(item.owner_id.def_id.to_def_id()) else {
            return;
        };
        let impl_self_ty = cx.tcx.type_of(impl_def_id).skip_binder();
        let Some(enum_def_id) = enum_def_id(impl_self_ty) else {
            return;
        };

        let body = cx.tcx.hir_body(*body_id);
        let name = item.ident.name.as_str();
        let trait_impl = item.impl_kind;

        if is_inherent(trait_impl) {
            if name == "as_str" && sig.decl.implicit_self != hir::ImplicitSelfKind::None {
                check_as_str(cx, item, body.value, enum_def_id);
            } else if matches!(name, "parse" | "from_stored" | "from_str") {
                check_parse(cx, item, body.value, enum_def_id);
            }
            return;
        }

        if name == "fmt" && is_display_impl(cx, impl_def_id) {
            check_display(cx, item, body.value, enum_def_id);
        } else if name == "from_str" && is_from_str_impl(cx, impl_def_id) {
            check_parse(cx, item, body.value, enum_def_id);
        }
    }
}

fn is_inherent(impl_kind: ImplItemImplKind) -> bool {
    matches!(impl_kind, ImplItemImplKind::Inherent { .. })
}

fn is_display_impl(cx: &LateContext<'_>, impl_def_id: hir::def_id::DefId) -> bool {
    is_core_trait_impl(cx, impl_def_id, CoreTrait::Display)
}

fn is_from_str_impl(cx: &LateContext<'_>, impl_def_id: hir::def_id::DefId) -> bool {
    is_core_trait_impl(cx, impl_def_id, CoreTrait::FromStr)
}

fn is_core_trait_impl(
    cx: &LateContext<'_>,
    impl_def_id: hir::def_id::DefId,
    trait_: CoreTrait,
) -> bool {
    let trait_ref = cx.tcx.impl_trait_ref(impl_def_id).instantiate_identity();
    trait_.matches_path(&cx.tcx.def_path_str(trait_ref.def_id))
}

enum CoreTrait {
    Display,
    FromStr,
}

impl CoreTrait {
    fn matches_path(&self, path: &str) -> bool {
        match self {
            Self::Display => path == "core::fmt::Display",
            Self::FromStr => matches!(path, "core::str::traits::FromStr" | "core::str::FromStr"),
        }
    }
}

fn enum_def_id(ty: Ty<'_>) -> Option<DefId> {
    let ty::Adt(def, _) = ty.peel_refs().kind() else {
        return None;
    };
    if !def.is_enum() {
        return None;
    }

    let field_counts = def.variants().iter().map(|variant| variant.fields.len());
    field_counts_are_fieldless(field_counts).then(|| def.did())
}

fn field_counts_are_fieldless(field_counts: impl IntoIterator<Item = usize>) -> bool {
    field_counts.into_iter().all(|field_count| field_count == 0)
}

fn check_as_str<'tcx>(
    cx: &LateContext<'tcx>,
    item: &'tcx ImplItem<'tcx>,
    body: &'tcx Expr<'tcx>,
    enum_def_id: DefId,
) {
    let Some(mapping) = enum_to_string_mapping(cx, body, enum_def_id) else {
        return;
    };
    emit_enum_to_string(
        cx,
        item.span,
        item.ident.name.as_str(),
        ConversionKind::AsStr,
        mapping,
    );
}

fn check_display<'tcx>(
    cx: &LateContext<'tcx>,
    item: &'tcx ImplItem<'tcx>,
    body: &'tcx Expr<'tcx>,
    enum_def_id: DefId,
) {
    let Some(mapping) = display_mapping(cx, body, enum_def_id) else {
        return;
    };
    emit_enum_to_string(cx, item.span, "fmt", ConversionKind::Display, mapping);
}

fn check_parse<'tcx>(
    cx: &LateContext<'tcx>,
    item: &'tcx ImplItem<'tcx>,
    body: &'tcx Expr<'tcx>,
    enum_def_id: DefId,
) {
    let Some(mapping) = string_to_enum_mapping(cx, body, enum_def_id) else {
        return;
    };
    emit_string_to_enum(cx, item.span, item.ident.name.as_str(), mapping);
}

fn display_mapping<'tcx>(
    cx: &LateContext<'tcx>,
    expr: &'tcx Expr<'tcx>,
    enum_def_id: DefId,
) -> Option<Mapping> {
    let expr = peel_block(expr)?;
    if expr.span.from_expansion() {
        return None;
    }

    if let Some(mapping) = write_str_match_arg(cx, expr, enum_def_id) {
        return Some(mapping);
    }

    match expr.kind {
        ExprKind::Match(scrutinee, arms, hir::MatchSource::Normal) => {
            enum_match_to_literal_bodies(cx, scrutinee, arms, enum_def_id, arm_write_str_literal)
        }
        _ => None,
    }
}

fn write_str_match_arg<'tcx>(
    cx: &LateContext<'tcx>,
    expr: &'tcx Expr<'tcx>,
    enum_def_id: DefId,
) -> Option<Mapping> {
    let ExprKind::MethodCall(segment, _receiver, args, _) = expr.kind else {
        return None;
    };
    if segment.ident.name.as_str() != "write_str" {
        return None;
    }
    let [arg] = args else {
        return None;
    };
    enum_to_string_mapping(cx, arg, enum_def_id)
}

fn enum_to_string_mapping<'tcx>(
    cx: &LateContext<'tcx>,
    expr: &'tcx Expr<'tcx>,
    enum_def_id: DefId,
) -> Option<Mapping> {
    let expr = peel_block(expr)?;
    if expr.span.from_expansion() {
        return None;
    }
    let ExprKind::Match(scrutinee, arms, hir::MatchSource::Normal) = expr.kind else {
        return None;
    };
    enum_match_to_literal_bodies(cx, scrutinee, arms, enum_def_id, arm_string_literal)
}

fn enum_match_to_literal_bodies<'tcx>(
    cx: &LateContext<'tcx>,
    scrutinee: &'tcx Expr<'tcx>,
    arms: &'tcx [hir::Arm<'tcx>],
    enum_def_id: DefId,
    literal: fn(&'tcx Expr<'tcx>) -> Option<String>,
) -> Option<Mapping> {
    if !is_self_scrutinee(scrutinee) {
        return None;
    }

    let mut mapping = Mapping::new();
    for arm in arms {
        if arm.span.from_expansion()
            || arm.guard.is_some()
            || !is_enum_variant_pat(cx, arm.pat, enum_def_id)
        {
            return None;
        }
        mapping.push(literal(arm.body)?);
    }

    mapping.non_empty()
}

fn string_to_enum_mapping<'tcx>(
    cx: &LateContext<'tcx>,
    expr: &'tcx Expr<'tcx>,
    enum_def_id: DefId,
) -> Option<Mapping> {
    let expr = peel_block(expr)?;
    if expr.span.from_expansion() {
        return None;
    }
    let ExprKind::Match(scrutinee, arms, hir::MatchSource::Normal) = expr.kind else {
        return None;
    };
    if !is_raw_string_input_scrutinee(scrutinee) {
        return None;
    }

    let mut mapping = Mapping::new();
    for arm in arms {
        if arm.span.from_expansion() || arm.guard.is_some() {
            return None;
        }
        if is_fallback_pat(arm.pat) {
            continue;
        }
        mapping.extend(pattern_string_literals(arm.pat)?);
        if !returns_enum_variant(cx, arm.body, enum_def_id) {
            return None;
        }
    }

    mapping.non_empty()
}

fn peel_block<'tcx>(expr: &'tcx Expr<'tcx>) -> Option<&'tcx Expr<'tcx>> {
    match expr.kind {
        ExprKind::Block(block, _) if block.stmts.is_empty() => block.expr,
        _ => Some(expr),
    }
}

fn is_self_scrutinee(expr: &Expr<'_>) -> bool {
    let Some(expr) = peel_block(expr) else {
        return false;
    };
    match expr.kind {
        ExprKind::Path(ref qpath) => qpath_is_self(qpath),
        ExprKind::Unary(hir::UnOp::Deref, inner) | ExprKind::AddrOf(_, _, inner) => {
            is_self_scrutinee(inner)
        }
        _ => false,
    }
}

fn is_raw_string_input_scrutinee(expr: &Expr<'_>) -> bool {
    let Some(expr) = peel_block(expr) else {
        return false;
    };
    match expr.kind {
        ExprKind::Path(_) => true,
        ExprKind::Unary(hir::UnOp::Deref, inner) | ExprKind::AddrOf(_, _, inner) => {
            is_raw_string_input_scrutinee(inner)
        }
        ExprKind::MethodCall(segment, receiver, args, _) => {
            args.is_empty()
                && is_string_view_method(segment.ident.name.as_str())
                && is_raw_string_input_base(receiver)
        }
        _ => false,
    }
}

fn is_raw_string_input_base(expr: &Expr<'_>) -> bool {
    let Some(expr) = peel_block(expr) else {
        return false;
    };
    match expr.kind {
        ExprKind::Path(_) => true,
        ExprKind::Unary(hir::UnOp::Deref, inner) | ExprKind::AddrOf(_, _, inner) => {
            is_raw_string_input_base(inner)
        }
        _ => false,
    }
}

fn is_string_view_method(name: &str) -> bool {
    matches!(name, "as_str" | "as_ref")
}

fn qpath_is_self(qpath: &hir::QPath<'_>) -> bool {
    match qpath {
        hir::QPath::Resolved(_, path) => path
            .segments
            .last()
            .is_some_and(|segment| segment.ident.name.as_str() == "self"),
        hir::QPath::TypeRelative(_, segment) => segment.ident.name.as_str() == "self",
    }
}

fn arm_string_literal(expr: &Expr<'_>) -> Option<String> {
    string_literal(peel_block(expr)?)
}

fn arm_write_str_literal(expr: &Expr<'_>) -> Option<String> {
    let expr = peel_block(expr)?;
    let ExprKind::MethodCall(segment, _receiver, args, _) = expr.kind else {
        return None;
    };
    if segment.ident.name.as_str() != "write_str" {
        return None;
    }
    let [arg] = args else {
        return None;
    };
    string_literal(arg)
}

fn string_literal(expr: &Expr<'_>) -> Option<String> {
    let expr = peel_block(expr)?;
    if expr.span.from_expansion() {
        return None;
    }
    let ExprKind::Lit(lit) = expr.kind else {
        return None;
    };
    match lit.node {
        LitKind::Str(symbol, _) => Some(symbol.as_str().to_string()),
        _ => None,
    }
}

fn pattern_string_literals(pat: &Pat<'_>) -> Option<Vec<String>> {
    match pat.kind {
        PatKind::Expr(pat_expr) => match pat_expr.kind {
            PatExprKind::Lit {
                lit,
                negated: false,
            } => match lit.node {
                LitKind::Str(symbol, _) => Some(vec![symbol.as_str().to_string()]),
                _ => None,
            },
            _ => None,
        },
        PatKind::Or(patterns) => patterns
            .iter()
            .map(pattern_string_literals)
            .collect::<Option<Vec<_>>>()
            .map(|groups| groups.into_iter().flatten().collect()),
        _ => None,
    }
}

fn is_enum_variant_pat(cx: &LateContext<'_>, pat: &Pat<'_>, enum_def_id: DefId) -> bool {
    match pat.kind {
        PatKind::TupleStruct(ref qpath, _, _) | PatKind::Struct(ref qpath, _, _) => cx
            .qpath_res(qpath, pat.hir_id)
            .opt_def_id()
            .is_some_and(|def_id| variant_belongs_to_enum(cx, def_id, enum_def_id)),
        PatKind::Expr(pat_expr) => match pat_expr.kind {
            PatExprKind::Path(ref qpath) => cx
                .qpath_res(qpath, pat_expr.hir_id)
                .opt_def_id()
                .is_some_and(|def_id| variant_belongs_to_enum(cx, def_id, enum_def_id)),
            _ => false,
        },
        PatKind::Or(patterns) => patterns
            .iter()
            .all(|pattern| is_enum_variant_pat(cx, pattern, enum_def_id)),
        _ => false,
    }
}

fn is_fallback_pat(pat: &Pat<'_>) -> bool {
    matches!(pat.kind, PatKind::Wild | PatKind::Binding(..))
}

fn returns_enum_variant(cx: &LateContext<'_>, expr: &Expr<'_>, enum_def_id: DefId) -> bool {
    let Some(expr) = peel_block(expr) else {
        return false;
    };
    match expr.kind {
        ExprKind::Path(ref qpath) => resolves_to_variant(cx, qpath, expr.hir_id, enum_def_id),
        ExprKind::Call(callee, args) => {
            resolves_to_result_or_option_ctor(cx, callee)
                && matches!(args, [arg] if returns_enum_variant(cx, arg, enum_def_id))
        }
        _ => false,
    }
}

fn resolves_to_variant(
    cx: &LateContext<'_>,
    qpath: &hir::QPath<'_>,
    hir_id: hir::HirId,
    enum_def_id: DefId,
) -> bool {
    cx.qpath_res(qpath, hir_id)
        .opt_def_id()
        .is_some_and(|def_id| variant_belongs_to_enum(cx, def_id, enum_def_id))
}

fn variant_belongs_to_enum(cx: &LateContext<'_>, def_id: DefId, enum_def_id: DefId) -> bool {
    let variant_def_id = match cx.tcx.def_kind(def_id) {
        DefKind::Ctor(..) => cx.tcx.parent(def_id),
        DefKind::Variant => def_id,
        _ => return false,
    };
    cx.tcx.opt_parent(variant_def_id) == Some(enum_def_id)
}

fn resolves_to_result_or_option_ctor(cx: &LateContext<'_>, expr: &Expr<'_>) -> bool {
    let ExprKind::Path(ref qpath) = expr.kind else {
        return false;
    };
    let Some(def_id) = cx.qpath_res(qpath, expr.hir_id).opt_def_id() else {
        return false;
    };
    is_lang_item_or_ctor(cx, def_id, LangItem::ResultOk)
        || is_lang_item_or_ctor(cx, def_id, LangItem::OptionSome)
}

fn emit_enum_to_string(
    cx: &LateContext<'_>,
    span: Span,
    name: &str,
    kind: ConversionKind,
    mapping: Mapping,
) {
    span_lint_and_then(
        cx,
        MANUAL_ENUM_STRING_CONVERSION,
        span,
        format!("`{name}` manually maps enum variants to string literals"),
        |diag| {
            diag.help(kind.help(mapping));
        },
    );
}

fn emit_string_to_enum(cx: &LateContext<'_>, span: Span, name: &str, mapping: Mapping) {
    span_lint_and_then(
        cx,
        MANUAL_ENUM_STRING_CONVERSION,
        span,
        format!("`{name}` manually maps string literals to enum variants"),
        |diag| {
            diag.help(format!(
                "derive `strum::EnumString` and preserve canonical/replay/stored strings with explicit `#[strum(serialize = ...)]` values{}",
                mapping.examples(),
            ));
        },
    );
}

enum ConversionKind {
    AsStr,
    Display,
}

impl ConversionKind {
    fn help(&self, mapping: Mapping) -> String {
        match self {
            Self::AsStr => format!(
                "derive `strum::AsRefStr` or `strum::IntoStaticStr` and use `#[strum(serialize = ...)]` for non-derived spellings{}; derived conversions are not `const`, so keep a manual `const fn as_str` only when const call sites require it; do not change canonical id, replay, or stored strings",
                mapping.examples(),
            ),
            Self::Display => format!(
                "derive `strum::Display` and use `#[strum(serialize = ...)]` for non-derived spellings{}",
                mapping.examples(),
            ),
        }
    }
}

struct Mapping {
    literals: Vec<String>,
}

impl Mapping {
    fn new() -> Self {
        Self {
            literals: Vec::new(),
        }
    }

    fn push(&mut self, literal: String) {
        self.literals.push(literal);
    }

    fn extend(&mut self, literals: Vec<String>) {
        self.literals.extend(literals);
    }

    fn non_empty(self) -> Option<Self> {
        (!self.literals.is_empty()).then_some(self)
    }

    fn examples(&self) -> String {
        let mut literals = self.literals.iter();
        let Some(first) = literals.next() else {
            return String::new();
        };
        let second = literals.next();
        match second {
            Some(second) => format!(", for example `{first}` or `{second}`"),
            None => format!(", for example `{first}`"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_trait_matches_display_path() {
        assert!(CoreTrait::Display.matches_path("core::fmt::Display"));
        assert!(!CoreTrait::Display.matches_path("core::str::traits::FromStr"));
    }

    #[test]
    fn core_trait_matches_from_str_paths() {
        assert!(CoreTrait::FromStr.matches_path("core::str::traits::FromStr"));
        assert!(CoreTrait::FromStr.matches_path("core::str::FromStr"));
        assert!(!CoreTrait::FromStr.matches_path("core::fmt::Display"));
    }

    #[test]
    fn mapping_examples_handles_empty_mapping() {
        assert_eq!(Mapping::new().examples(), "");
    }

    #[test]
    fn mapping_examples_mentions_one_literal() {
        let mut mapping = Mapping::new();
        mapping.push("acme".to_owned());

        assert_eq!(mapping.examples(), ", for example `acme`");
    }

    #[test]
    fn mapping_examples_mentions_first_two_literals() {
        let mut mapping = Mapping::new();
        mapping.extend(vec![
            "acme".to_owned(),
            "openlibrary".to_owned(),
            "manual".to_owned(),
        ]);

        assert_eq!(mapping.examples(), ", for example `acme` or `openlibrary`");
    }

    #[test]
    fn field_counts_accept_fieldless_enums() {
        assert!(field_counts_are_fieldless([0, 0, 0]));
    }

    #[test]
    fn field_counts_reject_data_carrying_enums() {
        assert!(!field_counts_are_fieldless([0, 1]));
        assert!(!field_counts_are_fieldless([2]));
    }

    #[test]
    fn string_view_methods_accept_only_borrowing_views() {
        assert!(is_string_view_method("as_str"));
        assert!(is_string_view_method("as_ref"));
        assert!(!is_string_view_method("to_lowercase"));
        assert!(!is_string_view_method("trim"));
    }

    #[test]
    fn mapping_non_empty_rejects_empty_mappings() {
        assert!(Mapping::new().non_empty().is_none());
    }
}
