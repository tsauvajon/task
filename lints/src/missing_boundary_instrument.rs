use clippy_utils::{diagnostics::span_lint_and_then, is_in_test, source::snippet_opt};
use rustc_hir::{self as hir, ImplItem, ImplItemImplKind, ImplItemKind, ImplicitSelfKind};
use rustc_lint::{LateContext, LateLintPass, LintContext};
use rustc_session::{declare_lint, impl_lint_pass};
use rustc_span::{BytePos, Span};

const TASK_CRATE: &str = "task";
const ATTRIBUTE_LOOKBACK_BYTES: u32 = 2_048;

declare_lint! {
    /// ### What it does
    /// Detects async task boundary methods without
    /// `#[tracing::instrument(skip_all, ...)]`.
    ///
    /// ### Why is this bad?
    /// Boundary spans are the most useful observability anchors, but omitting
    /// `skip_all` risks logging adapters, credentials, request bodies, or raw
    /// upstream payloads through generated tracing fields.
    pub MISSING_BOUNDARY_INSTRUMENT,
    Warn,
    "task boundary methods must use tracing::instrument(skip_all)"
}

#[derive(Copy, Clone)]
pub struct MissingBoundaryInstrument;

impl_lint_pass!(MissingBoundaryInstrument => [MISSING_BOUNDARY_INSTRUMENT]);

impl<'tcx> LateLintPass<'tcx> for MissingBoundaryInstrument {
    fn check_impl_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx ImplItem<'_>) {
        if is_in_test(cx.tcx, item.hir_id()) {
            return;
        }

        let ImplItemKind::Fn(sig, _) = &item.kind else {
            return;
        };

        if sig.decl.implicit_self == ImplicitSelfKind::None {
            return;
        }

        let item_snippet = snippet_opt(cx, item.span).unwrap_or_default();
        let signature_snippet = item_signature_snippet(&item_snippet);
        let trait_impl = matches!(item.impl_kind, ImplItemImplKind::Trait { .. });
        if !is_async_boundary_method(
            sig.header.asyncness.is_async(),
            trait_impl,
            item.span,
            signature_snippet,
        ) {
            return;
        }

        if !is_task_boundary(cx, item, trait_impl, signature_snippet) {
            return;
        }

        let instrument_snippet = item_snippet_with_attribute_prefix(cx, item.span, &item_snippet);
        match instrument_status(cx, item, &instrument_snippet) {
            InstrumentStatus::InstrumentedWithSkipAll => {}
            InstrumentStatus::InstrumentedWithoutSkipAll(span) => emit_missing_skip_all(cx, span),
            InstrumentStatus::Missing => {
                emit_missing_instrument(cx, item.ident.name.as_str(), item.span)
            }
        }
    }
}

enum InstrumentStatus {
    Missing,
    InstrumentedWithoutSkipAll(Span),
    InstrumentedWithSkipAll,
}

fn is_async_boundary_method(
    hir_async: bool,
    trait_impl: bool,
    span: Span,
    item_snippet: &str,
) -> bool {
    hir_async || contains_async_fn(item_snippet) || (trait_impl && span.from_expansion())
}

fn is_task_boundary(
    cx: &LateContext<'_>,
    item: &ImplItem<'_>,
    trait_impl: bool,
    item_snippet: &str,
) -> bool {
    if !is_in_task_crate(cx, item.owner_id.def_id) {
        return false;
    }

    trait_impl || source_declares_public_async_method(item_snippet)
}

fn is_in_task_crate(cx: &LateContext<'_>, local_def_id: hir::def_id::LocalDefId) -> bool {
    let def_id = local_def_id.to_def_id();
    cx.tcx.crate_name(def_id.krate).as_str() == TASK_CRATE
}

fn item_snippet_with_attribute_prefix(
    cx: &LateContext<'_>,
    span: Span,
    item_snippet: &str,
) -> String {
    let span = span.source_callsite();
    if let Some(prefix) = attribute_prefix_from_source_lines(cx, span) {
        return format!("{prefix}\n{item_snippet}");
    }

    let lo = span.lo();
    let lookback = lo.0.min(ATTRIBUTE_LOOKBACK_BYTES);
    let span = span.with_lo(lo - BytePos(lookback));
    snippet_opt(cx, span).unwrap_or_else(|| item_snippet.to_owned())
}

fn attribute_prefix_from_source_lines(cx: &LateContext<'_>, span: Span) -> Option<String> {
    let location = cx.sess().source_map().lookup_char_pos(span.lo());
    let mut line_index = location.line.checked_sub(1)?;
    let mut selected = Vec::new();
    let mut reverse_depth = 0;
    let mut saw_attribute_start = false;

    while line_index > 0 && selected.len() < 32 {
        line_index -= 1;
        let line = location.file.get_line(line_index)?;
        let line = line.as_ref();
        let trimmed = line.trim_start();
        if selected.is_empty() && trimmed.is_empty() {
            continue;
        }
        if selected.is_empty() && !could_end_attribute(trimmed) {
            return None;
        }
        if !selected.is_empty() && reverse_depth == 0 {
            if trimmed.is_empty() {
                selected.push(line.to_owned());
                continue;
            }
            if !could_end_attribute(trimmed) {
                break;
            }
        }

        reverse_depth -= bracket_delta(line);
        saw_attribute_start |= trimmed.starts_with("#[");
        selected.push(line.to_owned());
    }

    if !saw_attribute_start || reverse_depth != 0 {
        return None;
    }
    selected.reverse();
    Some(selected.join("\n"))
}

fn could_end_attribute(trimmed: &str) -> bool {
    trimmed.starts_with("#[") || trimmed.ends_with(']')
}

fn instrument_status(
    cx: &LateContext<'_>,
    item: &ImplItem<'_>,
    item_snippet: &str,
) -> InstrumentStatus {
    for attr in cx.tcx.hir_attrs(item.hir_id()) {
        if attr.doc_str().is_some() {
            continue;
        }
        if matches!(attr, hir::Attribute::Parsed(_)) {
            continue;
        }
        let span = attr.span();
        let Some(snippet) = snippet_opt(cx, span) else {
            continue;
        };
        if !looks_like_instrument_attribute(&snippet) {
            continue;
        }
        return if contains_identifier(&snippet, "skip_all") {
            InstrumentStatus::InstrumentedWithSkipAll
        } else {
            InstrumentStatus::InstrumentedWithoutSkipAll(span)
        };
    }

    let prefix = attribute_prefix_before_fn(item_snippet);
    if !looks_like_instrument_attribute(&prefix) {
        return InstrumentStatus::Missing;
    }
    if contains_identifier(&prefix, "skip_all") {
        InstrumentStatus::InstrumentedWithSkipAll
    } else {
        InstrumentStatus::InstrumentedWithoutSkipAll(item.span)
    }
}

fn emit_missing_instrument(cx: &LateContext<'_>, name: &str, span: Span) {
    span_lint_and_then(
        cx,
        MISSING_BOUNDARY_INSTRUMENT,
        span,
        format!("`{name}` is a task boundary without tracing instrumentation"),
        |diag| {
            diag.help(
                "add `#[tracing::instrument(skip_all, ...)]` with safe summary fields before the method",
            );
        },
    );
}

fn emit_missing_skip_all(cx: &LateContext<'_>, span: Span) {
    span_lint_and_then(
        cx,
        MISSING_BOUNDARY_INSTRUMENT,
        span,
        "task boundary instrumentation must use `skip_all`",
        |diag| {
            diag.help("add `skip_all` and record only explicit safe summary fields");
        },
    );
}

fn contains_async_fn(snippet: &str) -> bool {
    snippet.contains("async fn")
}

fn source_declares_public_async_method(snippet: &str) -> bool {
    let mut saw_public = false;
    let mut saw_async = false;
    for token in snippet.split_whitespace() {
        if token == "pub" || token.starts_with("pub(") {
            saw_public = true;
            continue;
        }
        if token == "async" {
            saw_async = true;
            continue;
        }
        if token == "fn" {
            return saw_public && saw_async;
        }
    }
    false
}

fn item_signature_snippet(snippet: &str) -> &str {
    snippet
        .split_once('{')
        .map_or(snippet, |(signature, _)| signature)
}

fn item_prefix_before_fn(snippet: &str) -> &str {
    let fn_index = fn_keyword_index(snippet);
    &snippet[..fn_index]
}

fn attribute_prefix_before_fn(snippet: &str) -> String {
    let prefix = item_prefix_before_fn(snippet);
    let Some((attribute_lines, _same_line_signature_prefix)) = prefix.rsplit_once('\n') else {
        return String::new();
    };
    let mut region_start = None;
    let mut attribute_depth = 0;
    let lines = attribute_lines.lines().collect::<Vec<_>>();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if region_start.is_none() {
            if trimmed.starts_with("#[") {
                region_start = Some(index);
                attribute_depth += bracket_delta(line);
            }
            continue;
        }

        if attribute_depth > 0 {
            attribute_depth += bracket_delta(line);
            continue;
        }
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with("#[") {
            attribute_depth += bracket_delta(line);
            continue;
        }
        region_start = None;
    }

    if attribute_depth != 0 {
        return String::new();
    }
    let Some(region_start) = region_start else {
        return String::new();
    };
    lines[region_start..].join("\n")
}

fn bracket_delta(line: &str) -> i32 {
    line.chars().fold(0, |depth, ch| match ch {
        '[' => depth + 1,
        ']' => depth - 1,
        _ => depth,
    })
}

fn fn_keyword_index(snippet: &str) -> usize {
    snippet
        .rfind("async fn")
        .or_else(|| snippet.rfind("fn"))
        .unwrap_or(snippet.len())
}

fn looks_like_instrument_attribute(snippet: &str) -> bool {
    snippet.lines().any(|line| {
        let compact = line
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .collect::<String>();
        compact.starts_with("#[tracing::instrument(") || compact.starts_with("#[instrument(")
    })
}

fn contains_identifier(snippet: &str, identifier: &str) -> bool {
    snippet.match_indices(identifier).any(|(start, _)| {
        let before = snippet[..start].chars().next_back();
        let after = snippet[start + identifier.len()..].chars().next();
        !before.is_some_and(is_identifier_char) && !after.is_some_and(is_identifier_char)
    })
}

fn is_identifier_char(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skip_all_match_requires_identifier_boundaries() {
        assert!(contains_identifier("#[instrument(skip_all)]", "skip_all"));
        assert!(!contains_identifier(
            "#[instrument(skip_all_fields)]",
            "skip_all"
        ));
    }

    #[test]
    fn attribute_detection_accepts_tracing_and_imported_forms() {
        assert!(looks_like_instrument_attribute(
            "#[tracing::instrument(skip_all)]"
        ));
        assert!(looks_like_instrument_attribute("#[instrument(skip_all)]"));
        assert!(!looks_like_instrument_attribute(
            "tracing::instrument(skip_all)"
        ));
        assert!(!looks_like_instrument_attribute("#[expect(unused)]"));
        assert!(!looks_like_instrument_attribute(
            "/// `#[tracing::instrument(skip_all)]`"
        ));
        assert!(!looks_like_instrument_attribute(
            "// #[tracing::instrument(skip_all)]"
        ));
    }

    #[test]
    fn prefix_stops_before_function_body() {
        let snippet = "#[tracing::instrument(skip_all)]\npub async fn execute(&self) { instrument(skip_all); }";
        assert_eq!(
            item_prefix_before_fn(snippet),
            "#[tracing::instrument(skip_all)]\npub "
        );
    }

    #[test]
    fn attribute_prefix_uses_only_current_item_attributes() {
        let snippet = r#"
#[tracing::instrument(skip_all)]
pub async fn previous(&self) {}

impl Example {
    #[tracing::instrument(skip_all)]
    pub async fn execute(&self) {}
}
"#;

        assert_eq!(
            attribute_prefix_before_fn(snippet),
            "    #[tracing::instrument(skip_all)]",
        );
    }

    #[test]
    fn attribute_prefix_keeps_multiline_attributes() {
        let snippet = r#"
impl Example {
    #[tracing::instrument(
        skip_all,
        name = "example.execute",
        fields(id = %id),
    )]
    pub async fn execute(&self, id: Id) {}
}
"#;

        assert_eq!(
            attribute_prefix_before_fn(snippet),
            r#"    #[tracing::instrument(
        skip_all,
        name = "example.execute",
        fields(id = %id),
    )]"#,
        );
    }

    #[test]
    fn attribute_prefix_does_not_reuse_previous_item_attributes() {
        let snippet = r#"
#[tracing::instrument(skip_all)]
pub async fn previous(&self) {}

impl Example {
    pub async fn execute(&self) {}
}
"#;

        assert_eq!(attribute_prefix_before_fn(snippet), "");
    }

    #[test]
    fn signature_snippet_excludes_function_body() {
        let snippet = "pub fn execute(&self) { let _ = \"pub async fn nested\"; }";
        assert_eq!(item_signature_snippet(snippet), "pub fn execute(&self) ");
    }

    #[test]
    fn public_async_detection_accepts_restricted_visibility_and_qualifiers() {
        assert!(source_declares_public_async_method(
            "pub async fn execute(&self)"
        ));
        assert!(source_declares_public_async_method(
            "pub(crate) async fn execute(&self)"
        ));
        assert!(source_declares_public_async_method(
            "pub(super) async unsafe fn execute(&self)"
        ));
        assert!(!source_declares_public_async_method(
            "async fn execute(&self)"
        ));
        assert!(!source_declares_public_async_method(
            "pub fn execute(&self)"
        ));
    }

    #[test]
    fn async_trait_expansion_counts_as_async_boundary_method() {
        assert!(is_async_boundary_method(true, false, Span::default(), ""));
        assert!(is_async_boundary_method(
            false,
            false,
            Span::default(),
            "pub async fn execute"
        ));
    }
}
