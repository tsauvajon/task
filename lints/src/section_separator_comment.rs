use std::collections::HashSet;

use clippy_utils::{diagnostics::span_lint_and_then, source::snippet_opt};
use rustc_hir::Item;
use rustc_lint::{LateContext, LateLintPass};
use rustc_session::{declare_lint, impl_lint_pass};
use rustc_span::BytePos;

declare_lint! {
    /// ### What it does
    /// Detects line comments that are formatted as section separators, such as
    /// `// --- parsing ---` or `// =====`.
    ///
    /// ### Why is this bad?
    /// Section separators usually mean a file or function has grown past its
    /// natural boundaries. Prefer named functions, modules, or plain comments
    /// that explain why the code exists.
    pub SECTION_SEPARATOR_COMMENT,
    Warn,
    "section separator comments should be replaced with names or modules"
}

#[derive(Default)]
pub struct SectionSeparatorComment {
    reported_starts: HashSet<BytePos>,
}

impl_lint_pass!(SectionSeparatorComment => [SECTION_SEPARATOR_COMMENT]);

impl<'tcx> LateLintPass<'tcx> for SectionSeparatorComment {
    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx Item<'tcx>) {
        if item.span.from_expansion() {
            return;
        }

        let Some(snippet) = snippet_opt(cx, item.span) else {
            return;
        };

        inspect_lines(cx, item.span.lo(), &snippet, &mut self.reported_starts);
    }
}

fn inspect_lines(
    cx: &LateContext<'_>,
    span_start: BytePos,
    snippet: &str,
    reported_starts: &mut HashSet<BytePos>,
) {
    let mut line_start = 0usize;
    for line in snippet.split_inclusive('\n') {
        let line_text = line.trim_end_matches('\n').trim_end_matches('\r');
        if let Some(comment_start) = section_separator_comment_start(line_text) {
            let start = span_start + BytePos((line_start + comment_start) as u32);
            if reported_starts.insert(start) {
                let end = span_start + BytePos((line_start + line_text.len()) as u32);
                lint_section_separator_comment(cx, rustc_span::Span::with_root_ctxt(start, end));
            }
        }

        line_start += line.len();
    }
}

fn section_separator_comment_start(line: &str) -> Option<usize> {
    let comment_start = line.len() - line.trim_start().len();
    let trimmed = &line[comment_start..];
    if !trimmed.starts_with("//") || trimmed.starts_with("///") || trimmed.starts_with("//!") {
        return None;
    }

    let body = trimmed[2..].trim_start();
    if starts_with_separator_run(body) {
        Some(comment_start)
    } else {
        None
    }
}

fn starts_with_separator_run(body: &str) -> bool {
    body.bytes().take_while(|byte| is_separator(*byte)).count() >= 3
}

fn is_separator(byte: u8) -> bool {
    matches!(byte, b'-' | b'=' | b'*' | b'_')
}

fn lint_section_separator_comment(cx: &LateContext<'_>, span: rustc_span::Span) {
    span_lint_and_then(
        cx,
        SECTION_SEPARATOR_COMMENT,
        span,
        "section separator comment hides structure in punctuation",
        |diag| {
            diag.help("replace the separator with a named helper/module, or use a plain explanatory comment");
        },
    );
}
