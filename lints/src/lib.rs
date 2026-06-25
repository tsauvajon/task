#![feature(rustc_private)]
#![deny(unfulfilled_lint_expectations)]
#![warn(unused_extern_crates)]

extern crate rustc_ast;
extern crate rustc_errors;
extern crate rustc_hir;
extern crate rustc_lint;
extern crate rustc_middle;
extern crate rustc_session;
extern crate rustc_span;

mod bumpy_road_function;
mod conditional_max_n_branches;
mod explicit_from;
mod item_order;
mod manual_enum_string_conversion;
mod missing_boundary_instrument;
mod naming;
mod no_unwrap_or_else_panic;
mod same_call_both_branches;
mod section_separator_comment;
mod shape_hash;
mod similar_fn_bodies;
mod similar_match_arms;

#[doc(hidden)]
#[unsafe(no_mangle)]
pub extern "C" fn register_lints(
    _sess: &rustc_session::Session,
    lint_store: &mut rustc_lint::LintStore,
) {
    lint_store.register_lints(&[
        bumpy_road_function::BUMPY_ROAD_FUNCTION,
        conditional_max_n_branches::CONDITIONAL_MAX_N_BRANCHES,
        explicit_from::EXPLICIT_FROM,
        item_order::PUB_USE_BEFORE_USE,
        item_order::TEST_MODULE_AT_END,
        item_order::USE_BEFORE_MOD,
        manual_enum_string_conversion::MANUAL_ENUM_STRING_CONVERSION,
        missing_boundary_instrument::MISSING_BOUNDARY_INSTRUMENT,
        no_unwrap_or_else_panic::NO_UNWRAP_OR_ELSE_PANIC,
        same_call_both_branches::SAME_CALL_BOTH_BRANCHES,
        section_separator_comment::SECTION_SEPARATOR_COMMENT,
        similar_fn_bodies::SIMILAR_FN_BODIES,
        similar_match_arms::SIMILAR_MATCH_ARMS,
    ]);
    lint_store.register_late_pass(|_| Box::new(bumpy_road_function::BumpyRoadFunction));
    lint_store
        .register_late_pass(|_| Box::new(conditional_max_n_branches::ConditionalMaxNBranches));
    lint_store.register_late_pass(|_| Box::new(explicit_from::ExplicitFrom));
    lint_store.register_late_pass(|_| Box::new(item_order::ItemOrder));
    lint_store.register_late_pass(|_| {
        Box::new(manual_enum_string_conversion::ManualEnumStringConversion)
    });
    lint_store
        .register_late_pass(|_| Box::new(missing_boundary_instrument::MissingBoundaryInstrument));
    lint_store.register_late_pass(|_| Box::new(no_unwrap_or_else_panic::NoUnwrapOrElsePanic));
    lint_store.register_late_pass(|_| Box::new(same_call_both_branches::SameCallBothBranches));
    lint_store.register_late_pass(|_| {
        Box::new(section_separator_comment::SectionSeparatorComment::default())
    });
    lint_store.register_late_pass(|_| Box::new(similar_fn_bodies::SimilarFnBodies::default()));
    lint_store.register_late_pass(|_| Box::new(similar_match_arms::SimilarMatchArms));
}

dylint_linting::dylint_library!();
