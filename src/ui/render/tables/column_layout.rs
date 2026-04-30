use super::cells::first_char_str;

/// A table column whose header and cell values can fold to a single
/// cell when the terminal is too narrow for the full form. The
/// compact form is always the first character of the full form —
/// both for the column header (`Tmux` → `T`, `Agent` → `A`) and for
/// every cell value (`open` → `o`, `busy` → `b`, etc.). `·` is
/// already a single character so it folds to itself.
pub(super) struct CompactableColumn {
    pub(super) full_header: &'static str,
    pub(super) full_width: u16,
}

impl CompactableColumn {
    /// The compact form always renders into exactly one cell.
    const COMPACT_WIDTH: u16 = 1;

    /// Header label to render at this column's current state.
    pub(super) fn header(&self, compact: bool) -> &'static str {
        if compact {
            first_char_str(self.full_header)
        } else {
            self.full_header
        }
    }

    /// Cell width to allocate for this column at its current state.
    pub(super) fn width(&self, compact: bool) -> u16 {
        if compact {
            Self::COMPACT_WIDTH
        } else {
            self.full_width
        }
    }

    /// Cells freed when this column folds from full to compact width.
    /// The inter-column gap is unaffected — only the column content
    /// shrinks.
    pub(super) fn compact_savings(&self) -> u16 {
        self.full_width.saturating_sub(Self::COMPACT_WIDTH)
    }
}

/// Tmux column. Full header `Tmux` (4 cells) sits over labels
/// `open` / `parked`; the column itself is 8 cells wide so the
/// widest label (`parked`, 6 cells) gets a 1-cell gutter on each
/// side. Compact form: `T` header, `o`/`p` labels, 1 cell wide.
pub(super) const TMUX_COLUMN: CompactableColumn = CompactableColumn {
    full_header: "Tmux",
    full_width: 8,
};

/// Agent column. Full header `Agent` (5 cells) sits over labels
/// `·` / `gone` / `busy` / `idle` / `hung`; 5 cells fits the header
/// exactly and leaves the data right-aligned with a 1-cell gutter
/// on its left. Compact form: `A` header, `·`/`g`/`b`/`i`/`h`
/// labels, 1 cell wide.
pub(super) const AGENT_COLUMN: CompactableColumn = CompactableColumn {
    full_header: "Agent",
    full_width: 5,
};

/// Width overhead added by the table chrome in the Tasks view: block
/// padding (1 left + 1 right), highlight symbol ("▶ " = 2 cells), plus
/// `column_count.saturating_sub(1)` cells of inter-column spacing.
pub(super) fn table_chrome_overhead(column_count: u16) -> u16 {
    // 2 (block padding) + 2 (highlight symbol)
    let fixed = 4u16;
    fixed.saturating_add(column_count.saturating_sub(1))
}

/// Layout decision for the Tasks table columns. Variants are ordered
/// from widest to narrowest:
///
/// 1. Repo + full branch (wide).
/// 2. Repo shortened to last `/`-segment + full branch (medium).
/// 3. Repo column hidden + full branch (narrow).
/// 4. Repo column hidden + short branch (last `/`-segment) (very
///    narrow).
/// 5. Repo column hidden + short branch + Tmux folded to one cell
///    (`T` header, `o`/`p` labels) (extremely narrow). Tmux folds
///    before disappearing so the user keeps a glanceable session
///    indicator for as long as possible.
/// 6. Repo column hidden + short branch + Tmux folded + Agent folded
///    to one cell (`A` header, `·`/`g`/`b`/`i`/`h` labels). Agent
///    folds after Tmux for the same reason: a glanceable indicator
///    is more useful than a missing column.
/// 7. Repo + Tmux columns hidden, short branch + Agent stays compact
///    (extremely narrow). Branch is the highest-priority piece of
///    information; Agent stays compact since once a column has been
///    compacted it never un-folds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TaskColumnLayout {
    RepoFullBranchFull,
    RepoShortBranchFull,
    NoRepoBranchFull,
    NoRepoBranchShort,
    NoRepoBranchShortTmuxCompact,
    NoRepoBranchShortAllCompact,
    NoRepoBranchShortNoTmux,
}

impl TaskColumnLayout {
    pub(super) fn shows_repo(self) -> bool {
        matches!(self, Self::RepoFullBranchFull | Self::RepoShortBranchFull)
    }

    pub(super) fn shortens_repo(self) -> bool {
        matches!(self, Self::RepoShortBranchFull)
    }

    pub(super) fn shortens_branch(self) -> bool {
        matches!(
            self,
            Self::NoRepoBranchShort
                | Self::NoRepoBranchShortTmuxCompact
                | Self::NoRepoBranchShortAllCompact
                | Self::NoRepoBranchShortNoTmux
        )
    }

    pub(super) fn shows_tmux(self) -> bool {
        !matches!(self, Self::NoRepoBranchShortNoTmux)
    }

    pub(super) fn compact_tmux(self) -> bool {
        matches!(
            self,
            Self::NoRepoBranchShortTmuxCompact | Self::NoRepoBranchShortAllCompact
        )
    }

    pub(super) fn compact_agent(self) -> bool {
        matches!(
            self,
            Self::NoRepoBranchShortAllCompact | Self::NoRepoBranchShortNoTmux
        )
    }
}

/// Pick the densest layout that still fits the available content
/// width. `content_width` is what's left after fixed-width columns
/// (Tmux, Agent) and table chrome are subtracted; it can be negative
/// when the terminal is narrower than the full-baseline layout, and
/// the deficit must be preserved so that compact-column savings are
/// not added to a clamped zero. With unsigned saturation a deficit
/// of e.g. `-3` would round up to `0` and let `compact_savings()`
/// over-count cells that don't actually exist, picking an
/// all-compact layout that still truncates the branch leaf.
pub(super) fn pick_task_column_layout(
    scoped: bool,
    content_width: i32,
    max_repo_full: u16,
    max_repo_short: u16,
    max_branch_full: u16,
    max_branch_short: u16,
) -> TaskColumnLayout {
    // Cells freed when each column folds from full to compact width.
    // Only the column content shrinks; the inter-column gap stays.
    let tmux_compact_savings = i32::from(TMUX_COLUMN.compact_savings());
    let agent_compact_savings = i32::from(AGENT_COLUMN.compact_savings());

    let max_repo_full = i32::from(max_repo_full);
    let max_repo_short = i32::from(max_repo_short);
    let max_branch_full = i32::from(max_branch_full);
    let max_branch_short = i32::from(max_branch_short);

    if scoped {
        // Scoped view never has a Repo column. The only question is
        // whether branches need shortening, and how much the Tmux
        // and Agent columns need to give up to keep the branch leaf
        // visible. Order of degradation: branch full → branch leaf
        // → Tmux compact → Agent compact → Tmux gone.
        if max_branch_full <= content_width {
            return TaskColumnLayout::NoRepoBranchFull;
        }
        if max_branch_short <= content_width {
            return TaskColumnLayout::NoRepoBranchShort;
        }
        if max_branch_short <= content_width + tmux_compact_savings {
            return TaskColumnLayout::NoRepoBranchShortTmuxCompact;
        }
        if max_branch_short <= content_width + tmux_compact_savings + agent_compact_savings {
            return TaskColumnLayout::NoRepoBranchShortAllCompact;
        }
        return TaskColumnLayout::NoRepoBranchShortNoTmux;
    }

    // Budget for Repo alongside a full-width Branch. `+ 1` accounts for
    // the column-spacing gap between Repo and Branch that lives inside
    // `content_width`.
    let repo_budget = content_width - (max_branch_full + 1);
    if max_repo_full <= repo_budget {
        return TaskColumnLayout::RepoFullBranchFull;
    }
    if max_repo_short <= repo_budget && repo_budget > 0 {
        return TaskColumnLayout::RepoShortBranchFull;
    }

    // Repo column will be dropped — one fewer inter-column gap is
    // consumed by the table chrome, so the Branch column can borrow
    // that cell back for its content budget.
    let no_repo_branch_budget = content_width + 1;
    if max_branch_full <= no_repo_branch_budget {
        return TaskColumnLayout::NoRepoBranchFull;
    }
    if max_branch_short <= no_repo_branch_budget {
        return TaskColumnLayout::NoRepoBranchShort;
    }
    // The branch leaf doesn't fit alongside the full Tmux column. Try
    // folding Tmux to a single cell before sacrificing it entirely —
    // a compact `T`/`o`/`p` indicator is still useful at a glance.
    if max_branch_short <= no_repo_branch_budget + tmux_compact_savings {
        return TaskColumnLayout::NoRepoBranchShortTmuxCompact;
    }
    // Compact Tmux still isn't enough. Fold the Agent column to a
    // single cell as well before dropping Tmux entirely — compact
    // `A`/`·`/`g`/`b`/`i`/`h` is still a useful glance indicator.
    if max_branch_short <= no_repo_branch_budget + tmux_compact_savings + agent_compact_savings {
        return TaskColumnLayout::NoRepoBranchShortAllCompact;
    }
    // Even compact Tmux + compact Agent can't keep the branch leaf in
    // frame. Drop Tmux entirely; Agent stays compact (once a column
    // has been compacted it never un-folds). Priority order is:
    // branch prefix > Tmux full > Agent full > Tmux compact > branch
    // leaf.
    TaskColumnLayout::NoRepoBranchShortNoTmux
}

#[cfg(test)]
mod tests {
    use super::*;

    mod compactable_column_tests {
        use super::*;

        #[test]
        fn header_returns_full_when_not_compact() {
            assert_eq!(TMUX_COLUMN.header(false), "Tmux");
            assert_eq!(AGENT_COLUMN.header(false), "Agent");
        }

        #[test]
        fn header_returns_first_character_when_compact() {
            assert_eq!(TMUX_COLUMN.header(true), "T");
            assert_eq!(AGENT_COLUMN.header(true), "A");
        }

        #[test]
        fn width_returns_full_when_not_compact() {
            assert_eq!(TMUX_COLUMN.width(false), 8);
            assert_eq!(AGENT_COLUMN.width(false), 5);
        }

        #[test]
        fn width_returns_one_when_compact() {
            assert_eq!(TMUX_COLUMN.width(true), 1);
            assert_eq!(AGENT_COLUMN.width(true), 1);
        }

        #[test]
        fn compact_savings_is_full_minus_one() {
            assert_eq!(TMUX_COLUMN.compact_savings(), 7);
            assert_eq!(AGENT_COLUMN.compact_savings(), 4);
        }

        #[test]
        fn compact_savings_saturates_when_full_width_is_at_or_below_compact() {
            // Pathological column declaring a full width of 0 must
            // not underflow. Real columns always declare full > 1,
            // but the helper should be robust.
            let degenerate = CompactableColumn {
                full_header: "X",
                full_width: 0,
            };
            assert_eq!(degenerate.compact_savings(), 0);
        }
    }

    mod pick_task_column_layout_tests {
        use super::{TaskColumnLayout, pick_task_column_layout};

        #[test]
        fn wide_unscoped_keeps_full_repo_and_branch() {
            // 80 cells of content space easily fits a 30-char repo
            // next to a 10-char branch with a 1-cell gap.
            let layout = pick_task_column_layout(
                /* scoped */ false, /* content_width */ 80, /* max_repo_full */ 30,
                /* max_repo_short */ 4, /* max_branch_full */ 10,
                /* max_branch_short */ 4,
            );
            assert_eq!(layout, TaskColumnLayout::RepoFullBranchFull);
        }

        #[test]
        fn shortens_repo_when_full_form_overflows() {
            // Room for branch (10) + gap (1) + short repo (4) = 15.
            // Full repo (30) doesn't fit; short form does.
            let layout = pick_task_column_layout(false, 20, 30, 4, 10, 4);
            assert_eq!(layout, TaskColumnLayout::RepoShortBranchFull);
        }

        #[test]
        fn drops_repo_column_when_even_short_form_does_not_fit() {
            // content 11 = just enough for branch (10) + gap (1). No
            // budget left for any repo form → hide the Repo column.
            let layout = pick_task_column_layout(false, 11, 30, 4, 10, 4);
            assert_eq!(layout, TaskColumnLayout::NoRepoBranchFull);
        }

        #[test]
        fn shortens_branch_when_no_repo_and_branch_overflows() {
            // content 8 — too narrow even for a full 10-char branch,
            // but the 4-char branch leaf still fits alongside Tmux.
            let layout = pick_task_column_layout(false, 8, 30, 4, 10, 4);
            assert_eq!(layout, TaskColumnLayout::NoRepoBranchShort);
        }

        #[test]
        fn folds_tmux_to_compact_when_branch_leaf_does_not_fit_with_full_tmux() {
            // content 3 (no-repo budget = 4) — too narrow for the
            // 10-char branch leaf alongside the full Tmux column.
            // Folding Tmux to its compact 1-cell form (savings = 7)
            // brings the budget up to 11, which fits the leaf.
            let layout = pick_task_column_layout(false, 3, 30, 4, 20, 10);
            assert_eq!(layout, TaskColumnLayout::NoRepoBranchShortTmuxCompact);
        }

        #[test]
        fn folds_agent_to_compact_when_compact_tmux_alone_is_not_enough() {
            // content 0 (no-repo budget = 1, compact-Tmux budget = 8)
            // — the 10-char leaf overflows even with compact Tmux.
            // Folding Agent to its compact 1-cell form (savings = 4)
            // brings the budget up to 12, which fits the leaf. Tmux
            // stays compact rather than disappearing.
            let layout = pick_task_column_layout(false, 0, 30, 4, 20, 10);
            assert_eq!(layout, TaskColumnLayout::NoRepoBranchShortAllCompact);
        }

        #[test]
        fn drops_tmux_when_branch_leaf_does_not_fit_even_with_both_compact() {
            // content 0 (no-repo budget = 1, all-compact budget = 12)
            // — a 14-char branch leaf overflows even with both Tmux
            // and Agent compact. Tmux disappears entirely; Agent
            // stays compact (a column never un-folds).
            let layout = pick_task_column_layout(false, 0, 30, 4, 20, 14);
            assert_eq!(layout, TaskColumnLayout::NoRepoBranchShortNoTmux);
        }

        #[test]
        fn keeps_full_tmux_when_branch_leaf_fits_with_full_tmux() {
            // content 9 (no-repo budget = 10) — exactly enough for
            // the 10-char branch leaf alongside the full Tmux column.
            let layout = pick_task_column_layout(false, 9, 30, 4, 20, 10);
            assert_eq!(layout, TaskColumnLayout::NoRepoBranchShort);
        }

        #[test]
        fn scoped_view_uses_full_branch_when_it_fits() {
            let layout = pick_task_column_layout(
                /* scoped */ true, /* content_width */ 20, /* max_repo_full */ 0,
                /* max_repo_short */ 0, /* max_branch_full */ 10,
                /* max_branch_short */ 4,
            );
            assert_eq!(layout, TaskColumnLayout::NoRepoBranchFull);
        }

        #[test]
        fn scoped_view_shortens_branch_when_it_does_not_fit() {
            let layout = pick_task_column_layout(true, 5, 0, 0, 10, 4);
            assert_eq!(layout, TaskColumnLayout::NoRepoBranchShort);
        }

        #[test]
        fn scoped_view_folds_tmux_to_compact_when_branch_leaf_does_not_fit() {
            // Scoped view, branch leaf is 10, content_width is 5.
            // Full Tmux: leaf overflows (5). Compact Tmux: budget =
            // 12, leaf fits.
            let layout = pick_task_column_layout(true, 5, 0, 0, 20, 10);
            assert_eq!(layout, TaskColumnLayout::NoRepoBranchShortTmuxCompact);
        }

        #[test]
        fn scoped_view_folds_agent_to_compact_when_compact_tmux_is_not_enough() {
            // Scoped view, branch leaf is 10, content_width is 1.
            // Compact-Tmux budget = 8, still doesn't fit the leaf.
            // Compact-Tmux + compact-Agent budget = 12, fits.
            let layout = pick_task_column_layout(true, 1, 0, 0, 20, 10);
            assert_eq!(layout, TaskColumnLayout::NoRepoBranchShortAllCompact);
        }

        #[test]
        fn scoped_view_drops_tmux_when_branch_leaf_does_not_fit_with_both_compact() {
            // Scoped view, branch leaf is 14, content_width is 1.
            // All-compact budget = 12, still doesn't fit the leaf.
            // Drop Tmux; Agent stays compact.
            let layout = pick_task_column_layout(true, 1, 0, 0, 20, 14);
            assert_eq!(layout, TaskColumnLayout::NoRepoBranchShortNoTmux);
        }

        #[test]
        fn zero_content_width_with_short_branch_leaf_folds_only_tmux_to_compact() {
            // Degenerate terminal: no-repo budget = 1, compact-Tmux
            // budget = 8. A 4-char branch leaf still fits alongside
            // the compact Tmux column without compacting Agent.
            let layout = pick_task_column_layout(false, 0, 30, 4, 10, 4);
            assert_eq!(layout, TaskColumnLayout::NoRepoBranchShortTmuxCompact);
        }

        #[test]
        fn negative_content_width_drops_tmux_when_branch_leaf_does_not_fit() {
            // Sub-baseline width: terminal is 3 cells short of the
            // full-baseline layout (e.g. unscoped width 17 with
            // chrome 7 + Tmux 8 + Agent 5 = 20). With the deficit
            // preserved, the no-repo all-compact budget is
            // -3 + 1 + 7 + 4 = 9, which is still short of the
            // 10-char branch leaf, so Tmux must drop. Saturating to
            // zero here would mistakenly pick all-compact and
            // truncate the leaf.
            let layout = pick_task_column_layout(false, -3, 30, 4, 20, 10);
            assert_eq!(layout, TaskColumnLayout::NoRepoBranchShortNoTmux);
        }

        #[test]
        fn scoped_negative_content_width_drops_tmux_when_branch_leaf_does_not_fit() {
            // Scoped variant of the same regression: deficit
            // -3, all-compact budget = -3 + 7 + 4 = 8, leaf 10.
            let layout = pick_task_column_layout(true, -3, 0, 0, 20, 10);
            assert_eq!(layout, TaskColumnLayout::NoRepoBranchShortNoTmux);
        }

        #[test]
        fn deeply_negative_content_width_still_drops_tmux_only_once() {
            // Even at extreme deficits, the worst-case layout is
            // NoRepoBranchShortNoTmux — we never panic or wrap on
            // signed arithmetic. Pin that with a wide enough
            // negative budget to demonstrate.
            let layout = pick_task_column_layout(false, -1000, 30, 4, 20, 10);
            assert_eq!(layout, TaskColumnLayout::NoRepoBranchShortNoTmux);
        }
    }
}
