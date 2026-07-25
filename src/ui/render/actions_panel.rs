use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::ui::{
    state::{InputMode, UiState, ViewMode, clamp_to_char_boundary},
    theme::Theme,
};

fn keybind_line(key: &str, desc: &str, key_color: Color, theme: &Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{key:<7}"), Style::default().fg(key_color)),
        Span::styled(format!(" {desc}"), theme.key_desc_style()),
    ])
}

fn input_line(
    label: &str,
    text: &str,
    cursor: usize,
    mode: InputMode,
    key_color: Color,
    theme: &Theme,
) -> Line<'static> {
    let text_style = Style::default().fg(key_color).add_modifier(Modifier::BOLD);
    let cursor = clamp_to_char_boundary(text, cursor);
    let mut spans = vec![Span::styled(label.to_owned(), theme.muted_style())];

    let before = text.get(..cursor).unwrap_or_default();
    if !before.is_empty() {
        spans.push(Span::styled(before.to_owned(), text_style));
    }

    let cursor_style = theme.cursor_style(mode);
    let Some(rest) = text.get(cursor..) else {
        spans.push(Span::styled(" ", cursor_style));
        return Line::from(spans);
    };
    let Some(cursor_char) = rest.chars().next() else {
        spans.push(Span::styled(" ", cursor_style));
        return Line::from(spans);
    };

    spans.push(Span::styled(cursor_char.to_string(), cursor_style));
    let suffix_start = cursor.saturating_add(cursor_char.len_utf8());
    let suffix = text.get(suffix_start..).unwrap_or_default();
    if !suffix.is_empty() {
        spans.push(Span::styled(suffix.to_owned(), text_style));
    }

    Line::from(spans)
}

#[derive(Clone, Copy)]
struct TextInputActions<'a> {
    submit_description: &'static str,
    label: &'static str,
    text: &'a str,
    cursor: usize,
    mode: InputMode,
}

fn text_input_actions(
    args: TextInputActions<'_>,
    key_color: Color,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let mut lines = vec![
        keybind_line("ctrl-u", "delete before cursor", key_color, theme),
        keybind_line("ctrl-k", "delete after cursor", key_color, theme),
        keybind_line("ctrl-a/e", "cursor start/end", key_color, theme),
        keybind_line("enter", args.submit_description, key_color, theme),
        keybind_line("esc", "return to normal", key_color, theme),
    ];
    lines.push(Line::from(""));
    lines.push(input_line(
        args.label,
        args.text,
        args.cursor,
        args.mode,
        key_color,
        theme,
    ));
    lines
}

pub(super) fn actions_for_mode(state: &UiState, theme: &Theme) -> Vec<Line<'static>> {
    let kc = theme.mode_color(state.mode);

    match state.mode {
        InputMode::Normal => match state.view {
            ViewMode::Tasks => {
                let mut lines = vec![
                    keybind_line("click", "select row / open selected", kc, theme),
                    keybind_line("enter", "open selected task", kc, theme),
                    keybind_line("tab", "switch to repos view", kc, theme),
                    keybind_line("/", "enter filter mode", kc, theme),
                    keybind_line("t", "create new task", kc, theme),
                    keybind_line("p", "park selected task", kc, theme),
                    keybind_line("f", finish_description(state), kc, theme),
                    keybind_line("ctrl-u/d", "half page up/down", kc, theme),
                    keybind_line("r", "refresh tasks", kc, theme),
                    keybind_line("ctrl+p", "commands", kc, theme),
                    keybind_line("q", "quit", kc, theme),
                ];
                if state.task_repo_scope.is_some() {
                    lines.insert(0, keybind_line("esc", "back to repos", kc, theme));
                }
                lines
            }
            ViewMode::Repos => vec![
                keybind_line("click", "select row / create task", kc, theme),
                keybind_line("enter", "view selected repo tasks", kc, theme),
                keybind_line("tab", "switch to tasks view", kc, theme),
                keybind_line("/", "enter filter mode", kc, theme),
                keybind_line("t", "create new task", kc, theme),
                keybind_line("c", "clone repo interactively", kc, theme),
                keybind_line("d", "toggle detached worktree", kc, theme),
                keybind_line("ctrl-u/d", "half page up/down", kc, theme),
                keybind_line("r", "refresh repos", kc, theme),
                keybind_line("ctrl+p", "commands", kc, theme),
                keybind_line("q", "quit", kc, theme),
            ],
        },
        InputMode::Filter => {
            let mut lines = vec![
                keybind_line("tab", "switch tasks/repos", kc, theme),
                keybind_line("ctrl-a/e", "cursor start/end", kc, theme),
                keybind_line("ctrl-u", "delete before cursor", kc, theme),
                keybind_line("ctrl-k", "delete after cursor", kc, theme),
                keybind_line("enter", "apply and return", kc, theme),
                keybind_line("esc", "return to normal", kc, theme),
            ];
            lines.push(Line::from(""));
            lines.push(input_line(
                "filter ",
                &state.filter_text,
                state.filter_cursor,
                InputMode::Filter,
                kc,
                theme,
            ));
            lines
        }
        InputMode::CreateTask => text_input_actions(
            TextInputActions {
                submit_description: "create and open task",
                label: "branch ",
                text: &state.create_branch,
                cursor: state.create_cursor,
                mode: InputMode::CreateTask,
            },
            kc,
            theme,
        ),
        InputMode::CloneRepo => text_input_actions(
            TextInputActions {
                submit_description: "clone repository",
                label: "url ",
                text: &state.clone_input,
                cursor: state.clone_cursor,
                mode: InputMode::CloneRepo,
            },
            kc,
            theme,
        ),
    }
}

fn finish_description(state: &UiState) -> &'static str {
    if state.pending_force_finish_matches_selected_task() {
        "force finish selected task"
    } else {
        "finish selected task"
    }
}

#[cfg(test)]
mod tests {
    use super::actions_for_mode;
    use crate::ui::{
        state::{InputMode, UiState, ViewMode},
        theme::Theme,
    };

    fn state_with_mode(mode: InputMode, view: ViewMode) -> UiState {
        let mut state = UiState::new(Vec::new(), Vec::new(), None);
        state.view = view;
        state.mode = mode;
        state
    }

    fn input_span_texts(lines: &[ratatui::text::Line<'static>], label: &str) -> Vec<String> {
        lines
            .iter()
            .find(|line| line.to_string().starts_with(label))
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.to_string())
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn normal_tasks_lists_task_actions() {
        let theme = Theme::dark();
        let state = state_with_mode(InputMode::Normal, ViewMode::Tasks);
        let lines = actions_for_mode(&state, &theme);
        let text: String = lines.iter().map(std::string::ToString::to_string).collect();
        assert!(text.contains("open selected task"));
        assert!(text.contains("park"));
        assert!(text.contains("finish"));
        assert!(text.contains("create new task"));
        assert!(text.contains("half page up/down"));
    }

    #[test]
    fn pending_force_finish_updates_finish_action() {
        use std::path::PathBuf;

        use crate::{
            runtime::{
                BranchName, RepoKey,
                task_rows::{TaskRow, TaskStatus},
            },
            tools::opencode::status::OpenCodeState,
        };

        let theme = Theme::dark();
        let row = TaskRow {
            status: TaskStatus::Open,
            repo: RepoKey::new("github.com/acme/app"),
            branch: BranchName::new("dirty-task"),
            worktree_name: "dirty-task".to_owned(),
            path: PathBuf::from("/tmp/github.com/acme/app/dirty-task"),
            opencode: OpenCodeState::None,
        };
        let mut state = UiState::new(vec![row], Vec::new(), None);
        assert!(state.set_pending_force_finish_to_selected_task());

        let lines = actions_for_mode(&state, &theme);
        let text: String = lines.iter().map(std::string::ToString::to_string).collect();

        assert!(text.contains("force finish selected task"));
    }

    #[test]
    fn normal_repos_lists_repo_actions() {
        let theme = Theme::dark();
        let state = state_with_mode(InputMode::Normal, ViewMode::Repos);
        let lines = actions_for_mode(&state, &theme);
        let text: String = lines.iter().map(std::string::ToString::to_string).collect();
        assert!(text.contains("clone repo"));
        assert!(text.contains("switch to tasks view"));
        assert!(text.contains("half page up/down"));
    }

    #[test]
    fn filter_mode_lists_filter_actions() {
        let theme = Theme::dark();
        let state = state_with_mode(InputMode::Filter, ViewMode::Tasks);
        let lines = actions_for_mode(&state, &theme);
        let text: String = lines.iter().map(std::string::ToString::to_string).collect();
        assert!(text.contains("ctrl-u") && text.contains("delete before cursor"));
        assert!(text.contains("ctrl-k") && text.contains("delete after cursor"));
        assert!(text.contains("cursor start/end"));
        assert!(text.contains("esc"));
    }

    #[test]
    fn create_task_mode_lists_create_actions() {
        let theme = Theme::dark();
        let state = state_with_mode(InputMode::CreateTask, ViewMode::Tasks);
        let lines = actions_for_mode(&state, &theme);
        let text: String = lines.iter().map(std::string::ToString::to_string).collect();
        assert!(text.contains("ctrl-u") && text.contains("delete before cursor"));
        assert!(text.contains("ctrl-k") && text.contains("delete after cursor"));
        assert!(text.contains("cursor start/end"));
        assert!(text.contains("create and open"));
    }

    #[test]
    fn clone_repo_mode_lists_clone_actions() {
        let theme = Theme::dark();
        let state = state_with_mode(InputMode::CloneRepo, ViewMode::Repos);
        let lines = actions_for_mode(&state, &theme);
        let text: String = lines.iter().map(std::string::ToString::to_string).collect();
        assert!(text.contains("ctrl-u") && text.contains("delete before cursor"));
        assert!(text.contains("ctrl-k") && text.contains("delete after cursor"));
        assert!(text.contains("cursor start/end"));
        assert!(text.contains("clone repository"));
    }

    #[test]
    fn clone_repo_mode_interpolates_input_text() {
        let theme = Theme::dark();
        let mut state = state_with_mode(InputMode::CloneRepo, ViewMode::Repos);
        state.clone_input = "git@github.com:me/app.git".to_owned();
        let lines = actions_for_mode(&state, &theme);
        let text: String = lines.iter().map(std::string::ToString::to_string).collect();
        assert!(
            text.contains("git@github.com:me/app.git"),
            "clone_input should appear in actions: {text}"
        );
    }

    #[test]
    fn filter_mode_same_actions_regardless_of_view() {
        let theme = Theme::dark();
        let tasks_state = state_with_mode(InputMode::Filter, ViewMode::Tasks);
        let repos_state = state_with_mode(InputMode::Filter, ViewMode::Repos);
        let tasks_lines: Vec<String> = actions_for_mode(&tasks_state, &theme)
            .iter()
            .map(std::string::ToString::to_string)
            .collect();
        let repos_lines: Vec<String> = actions_for_mode(&repos_state, &theme)
            .iter()
            .map(std::string::ToString::to_string)
            .collect();
        assert_eq!(
            tasks_lines, repos_lines,
            "Filter mode actions should be identical regardless of view"
        );
    }

    #[test]
    fn normal_tasks_does_not_include_repo_specific_actions() {
        let theme = Theme::dark();
        let state = state_with_mode(InputMode::Normal, ViewMode::Tasks);
        let lines = actions_for_mode(&state, &theme);
        let text: String = lines.iter().map(std::string::ToString::to_string).collect();
        assert!(
            !text.contains("clone repo"),
            "tasks view should not include clone action: {text}"
        );
    }

    #[test]
    fn normal_repos_does_not_include_task_specific_actions() {
        let theme = Theme::dark();
        let state = state_with_mode(InputMode::Normal, ViewMode::Repos);
        let lines = actions_for_mode(&state, &theme);
        let text: String = lines.iter().map(std::string::ToString::to_string).collect();
        assert!(
            !text.contains("park"),
            "repos view should not include park action: {text}"
        );
        assert!(
            !text.contains("finish"),
            "repos view should not include finish action: {text}"
        );
    }

    #[test]
    fn normal_repos_includes_create_new_task_action() {
        let theme = Theme::dark();
        let state = state_with_mode(InputMode::Normal, ViewMode::Repos);
        let lines = actions_for_mode(&state, &theme);
        let text: String = lines.iter().map(std::string::ToString::to_string).collect();
        assert!(
            text.contains("create new task"),
            "repos view should include create new task action: {text}"
        );
    }

    #[test]
    fn normal_repos_includes_detach_action() {
        let theme = Theme::dark();
        let state = state_with_mode(InputMode::Normal, ViewMode::Repos);
        let lines = actions_for_mode(&state, &theme);
        let text: String = lines.iter().map(std::string::ToString::to_string).collect();
        assert!(
            text.contains("toggle detached worktree"),
            "repos view should include detach action: {text}"
        );
    }

    #[test]
    fn create_task_mode_shows_branch_label_when_empty() {
        let theme = Theme::dark();
        let state = state_with_mode(InputMode::CreateTask, ViewMode::Tasks);
        assert!(state.create_branch.is_empty());
        let lines = actions_for_mode(&state, &theme);
        let text: String = lines.iter().map(std::string::ToString::to_string).collect();
        assert!(
            text.contains("branch"),
            "branch label should appear even when branch is empty: {text}"
        );
    }

    #[test]
    fn create_task_mode_shows_typed_branch_text() {
        let theme = Theme::dark();
        let mut state = state_with_mode(InputMode::CreateTask, ViewMode::Tasks);
        state.create_branch = "feat/my-feature".to_owned();
        let lines = actions_for_mode(&state, &theme);
        let text: String = lines.iter().map(std::string::ToString::to_string).collect();
        assert!(
            text.contains("feat/my-feature"),
            "typed branch name should appear in actions: {text}"
        );
    }

    #[test]
    fn filter_mode_renders_cursor_inside_input() {
        let theme = Theme::dark();
        let mut state = state_with_mode(InputMode::Filter, ViewMode::Tasks);
        state.filter_text = "abc".to_owned();
        state.filter_cursor = 1;

        let lines = actions_for_mode(&state, &theme);
        let spans = input_span_texts(&lines, "filter ");

        assert_eq!(spans, vec!["filter ", "a", "b", "c"]);
    }

    #[test]
    fn create_task_mode_renders_blank_cursor_at_input_end() {
        let theme = Theme::dark();
        let mut state = state_with_mode(InputMode::CreateTask, ViewMode::Tasks);
        state.create_branch = "feat".to_owned();
        state.create_cursor = state.create_branch.len();

        let lines = actions_for_mode(&state, &theme);
        let spans = input_span_texts(&lines, "branch ");

        assert_eq!(spans, vec!["branch ", "feat", " "]);
    }

    #[test]
    fn clone_repo_mode_renders_cursor_inside_input() {
        let theme = Theme::dark();
        let mut state = state_with_mode(InputMode::CloneRepo, ViewMode::Repos);
        state.clone_input = "repo".to_owned();
        state.clone_cursor = 2;

        let lines = actions_for_mode(&state, &theme);
        let spans = input_span_texts(&lines, "url ");

        assert_eq!(spans, vec!["url ", "re", "p", "o"]);
    }

    #[test]
    fn create_task_mode_shows_kill_actions() {
        let theme = Theme::dark();
        let state = state_with_mode(InputMode::CreateTask, ViewMode::Tasks);
        let lines = actions_for_mode(&state, &theme);
        let text: String = lines.iter().map(std::string::ToString::to_string).collect();
        assert!(
            text.contains("ctrl-u")
                && text.contains("delete before cursor")
                && text.contains("ctrl-k")
                && text.contains("delete after cursor"),
            "create task mode should include both kill actions: {text}"
        );
    }

    #[test]
    fn scoped_tasks_view_includes_esc_action() {
        let theme = Theme::dark();
        let mut state = state_with_mode(InputMode::Normal, ViewMode::Tasks);
        state.task_repo_scope = Some("github.com/acme/app".to_owned());
        let lines = actions_for_mode(&state, &theme);
        let text: String = lines.iter().map(std::string::ToString::to_string).collect();
        assert!(
            text.contains("back to repos"),
            "scoped tasks view should include esc/back to repos: {text}"
        );
    }

    #[test]
    fn unscoped_tasks_view_excludes_esc_action() {
        let theme = Theme::dark();
        let state = state_with_mode(InputMode::Normal, ViewMode::Tasks);
        assert!(state.task_repo_scope.is_none());
        let lines = actions_for_mode(&state, &theme);
        let text: String = lines.iter().map(std::string::ToString::to_string).collect();
        assert!(
            !text.contains("back to repos"),
            "unscoped tasks view should not include back to repos: {text}"
        );
    }
}
