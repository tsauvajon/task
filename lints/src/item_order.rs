use clippy_utils::{diagnostics::span_lint_and_then, is_in_test, source::snippet_opt};
use rustc_hir::{HirId, Item, ItemKind, Mod};
use rustc_lint::{LateContext, LateLintPass};
use rustc_session::{declare_lint, impl_lint_pass};

declare_lint! {
    /// ### What it does
    /// Detects `use` and `pub use` items declared after `mod` items in the same
    /// module.
    ///
    /// ### Why is this bad?
    /// Imports are easier to scan when they are grouped before local module
    /// declarations.
    pub USE_BEFORE_MOD,
    Warn,
    "imports should appear before module declarations"
}

declare_lint! {
    /// ### What it does
    /// Detects `pub use` and restricted-public `use` items declared after
    /// private `use` items in the same module.
    ///
    /// ### Why is this bad?
    /// Re-exports define the public surface of a module and are easier to scan
    /// when they are grouped before private implementation imports.
    pub PUB_USE_BEFORE_USE,
    Warn,
    "re-exports should appear before private imports"
}

declare_lint! {
    /// ### What it does
    /// Detects test modules that appear before non-test items in the same
    /// module.
    ///
    /// ### Why is this bad?
    /// Keeping tests at the bottom keeps the production code path readable
    /// without interleaving test-only code and implementation items.
    pub TEST_MODULE_AT_END,
    Warn,
    "test modules should be the last items in their module"
}

#[derive(Copy, Clone)]
pub struct ItemOrder;

impl_lint_pass!(ItemOrder => [PUB_USE_BEFORE_USE, TEST_MODULE_AT_END, USE_BEFORE_MOD]);

impl<'tcx> LateLintPass<'tcx> for ItemOrder {
    fn check_mod(&mut self, cx: &LateContext<'tcx>, module: &'tcx Mod<'tcx>, id: HirId) {
        let mut first_mod = None;
        let mut first_private_use = None;
        let mut first_test_module = None;
        let enforce_test_module_at_end = !is_in_test(cx.tcx, id);
        for item in module.item_ids.iter().map(|&id| cx.tcx.hir_item(id)) {
            if item.span.from_expansion() {
                continue;
            }

            if enforce_test_module_at_end
                && let Some(first_test_module) = first_test_module
                && !is_test_module(cx, item)
            {
                lint_test_module_before_item(cx, first_test_module, item);
            }

            match item.kind {
                ItemKind::Mod(..) => {
                    if is_test_module(cx, item) {
                        first_test_module.get_or_insert(item);
                    }
                    first_mod.get_or_insert(item);
                }
                ItemKind::Use(..) => {
                    let Some(import_visibility) = import_visibility(cx, item) else {
                        continue;
                    };
                    match import_visibility {
                        ImportVisibility::Public => {
                            if let Some(first_private_use) = first_private_use {
                                lint_pub_use_after_use(cx, item, first_private_use);
                            }
                        }
                        ImportVisibility::Private => {
                            first_private_use.get_or_insert(item);
                        }
                    }

                    if let Some(first_mod) = first_mod {
                        lint_use_after_mod(cx, item, first_mod);
                    }
                }
                _ => {}
            }
        }
    }
}

#[derive(Copy, Clone)]
enum ImportVisibility {
    Public,
    Private,
}

fn import_visibility(cx: &LateContext<'_>, item: &Item<'_>) -> Option<ImportVisibility> {
    snippet_opt(cx, item.span).and_then(|snippet| {
        let snippet = snippet.trim_start();
        if snippet.starts_with("use ") {
            Some(ImportVisibility::Private)
        } else if snippet.starts_with("pub ") || snippet.starts_with("pub(") {
            Some(ImportVisibility::Public)
        } else {
            None
        }
    })
}

fn is_test_module(cx: &LateContext<'_>, item: &Item<'_>) -> bool {
    if !matches!(item.kind, ItemKind::Mod(..)) {
        return false;
    }

    snippet_opt(cx, item.span)
        .is_some_and(|snippet| snippet.contains("#[cfg(test)]") && snippet.contains("mod tests"))
}

fn lint_use_after_mod(cx: &LateContext<'_>, use_item: &Item<'_>, first_mod: &Item<'_>) {
    span_lint_and_then(
        cx,
        USE_BEFORE_MOD,
        use_item.span,
        "import appears after a module declaration",
        |diag| {
            diag.span_note(first_mod.span, "first module declaration is here");
            diag.help("move `use` and `pub use` items before `mod` and `pub mod` items");
        },
    );
}

fn lint_pub_use_after_use(
    cx: &LateContext<'_>,
    public_use_item: &Item<'_>,
    first_private_use: &Item<'_>,
) {
    span_lint_and_then(
        cx,
        PUB_USE_BEFORE_USE,
        public_use_item.span,
        "re-export appears after a private import",
        |diag| {
            diag.span_note(first_private_use.span, "first private import is here");
            diag.help("move `pub use` items before private `use` items");
        },
    );
}

fn lint_test_module_before_item(
    cx: &LateContext<'_>,
    test_module: &Item<'_>,
    later_item: &Item<'_>,
) {
    span_lint_and_then(
        cx,
        TEST_MODULE_AT_END,
        test_module.span,
        "test module appears before a non-test item",
        |diag| {
            diag.span_note(later_item.span, "later non-test item is here");
            diag.help("move `#[cfg(test)] mod tests` to the end of the module");
        },
    );
}
