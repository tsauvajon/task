use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::ui::{
    state::{InputMode, UiState, ViewMode},
    theme::Theme,
};

fn keybind_line(key: &str, desc: &str, key_color: Color, theme: &Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{key:<7}"), Style::default().fg(key_color)),
        Span::styled(format!(" {desc}"), theme.key_desc_style()),
    ])
}

pub(super) fn actions_for_mode(state: &UiState, theme: &Theme) -> Vec<Line<'static>> {
    let kc = theme.mode_color(state.mode);

    match state.mode {
        InputMode::Normal => match state.view {
            ViewMode::Tasks => {
                let mut lines = vec![
                    keybind_line("enter", "open selected task", kc, theme),
                    keybind_line("tab", "switch to repos view", kc, theme),
                    keybind_line("/", "enter filter mode", kc, theme),
                    keybind_line("t", "create new task", kc, theme),
                    keybind_line("p", "park selected task", kc, theme),
                    keybind_line("f", "finish selected task", kc, theme),
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
                keybind_line("enter", "view selected repo tasks", kc, theme),
                keybind_line("tab", "switch to tasks view", kc, theme),
                keybind_line("/", "enter filter mode", kc, theme),
                keybind_line("t", "create new task", kc, theme),
                keybind_line("c", "clone repo interactively", kc, theme),
                keybind_line("d", "toggle detached worktree", kc, theme),
                keybind_line("r", "refresh repos", kc, theme),
                keybind_line("ctrl+p", "commands", kc, theme),
                keybind_line("q", "quit", kc, theme),
            ],
        },
        InputMode::Filter => {
            let mut lines = vec![
                keybind_line("tab", "switch tasks/repos", kc, theme),
                keybind_line("ctrl-u", "clear filter", kc, theme),
                keybind_line("enter", "apply and return", kc, theme),
                keybind_line("esc", "return to normal", kc, theme),
            ];
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("filter ", theme.muted_style()),
                Span::styled(
                    state.filter_text.clone(),
                    Style::default().fg(kc).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" ", theme.cursor_style(InputMode::Filter)),
            ]));
            lines
        }
        InputMode::CreateTask => {
            let mut lines = vec![
                keybind_line("ctrl-u", "clear branch name", kc, theme),
                keybind_line("enter", "create and open task", kc, theme),
                keybind_line("esc", "return to normal", kc, theme),
            ];
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("branch ", theme.muted_style()),
                Span::styled(
                    state.create_branch.clone(),
                    Style::default().fg(kc).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" ", theme.cursor_style(InputMode::CreateTask)),
            ]));
            lines
        }
        InputMode::CloneRepo => {
            let mut lines = vec![
                keybind_line("ctrl-u", "clear input", kc, theme),
                keybind_line("enter", "clone repository", kc, theme),
                keybind_line("esc", "return to normal", kc, theme),
            ];
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("url ", theme.muted_style()),
                Span::styled(
                    state.clone_input.clone(),
                    Style::default().fg(kc).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" ", theme.cursor_style(InputMode::CloneRepo)),
            ]));
            lines
        }
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

    #[test]
    fn normal_tasks_lists_task_actions() {
        let theme = Theme::dark();
        let state = state_with_mode(InputMode::Normal, ViewMode::Tasks);
        let lines = actions_for_mode(&state, &theme);
        let text: String = lines.iter().map(|l| l.to_string()).collect();
        assert!(text.contains("open selected task"));
        assert!(text.contains("park"));
        assert!(text.contains("finish"));
        assert!(text.contains("create new task"));
    }

    #[test]
    fn normal_repos_lists_repo_actions() {
        let theme = Theme::dark();
        let state = state_with_mode(InputMode::Normal, ViewMode::Repos);
        let lines = actions_for_mode(&state, &theme);
        let text: String = lines.iter().map(|l| l.to_string()).collect();
        assert!(text.contains("clone repo"));
        assert!(text.contains("switch to tasks view"));
    }

    #[test]
    fn filter_mode_lists_filter_actions() {
        let theme = Theme::dark();
        let state = state_with_mode(InputMode::Filter, ViewMode::Tasks);
        let lines = actions_for_mode(&state, &theme);
        let text: String = lines.iter().map(|l| l.to_string()).collect();
        assert!(text.contains("clear filter"));
        assert!(text.contains("esc"));
    }

    #[test]
    fn create_task_mode_lists_create_actions() {
        let theme = Theme::dark();
        let state = state_with_mode(InputMode::CreateTask, ViewMode::Tasks);
        let lines = actions_for_mode(&state, &theme);
        let text: String = lines.iter().map(|l| l.to_string()).collect();
        assert!(text.contains("clear branch name"));
        assert!(text.contains("create and open"));
    }

    #[test]
    fn clone_repo_mode_lists_clone_actions() {
        let theme = Theme::dark();
        let state = state_with_mode(InputMode::CloneRepo, ViewMode::Repos);
        let lines = actions_for_mode(&state, &theme);
        let text: String = lines.iter().map(|l| l.to_string()).collect();
        assert!(text.contains("clear input"));
        assert!(text.contains("clone repository"));
    }

    #[test]
    fn clone_repo_mode_interpolates_input_text() {
        let theme = Theme::dark();
        let mut state = state_with_mode(InputMode::CloneRepo, ViewMode::Repos);
        state.clone_input = "git@github.com:me/app.git".to_string();
        let lines = actions_for_mode(&state, &theme);
        let text: String = lines.iter().map(|l| l.to_string()).collect();
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
            .map(|l| l.to_string())
            .collect();
        let repos_lines: Vec<String> = actions_for_mode(&repos_state, &theme)
            .iter()
            .map(|l| l.to_string())
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
        let text: String = lines.iter().map(|l| l.to_string()).collect();
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
        let text: String = lines.iter().map(|l| l.to_string()).collect();
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
        let text: String = lines.iter().map(|l| l.to_string()).collect();
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
        let text: String = lines.iter().map(|l| l.to_string()).collect();
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
        let text: String = lines.iter().map(|l| l.to_string()).collect();
        assert!(
            text.contains("branch"),
            "branch label should appear even when branch is empty: {text}"
        );
    }

    #[test]
    fn create_task_mode_shows_typed_branch_text() {
        let theme = Theme::dark();
        let mut state = state_with_mode(InputMode::CreateTask, ViewMode::Tasks);
        state.create_branch = "feat/my-feature".to_string();
        let lines = actions_for_mode(&state, &theme);
        let text: String = lines.iter().map(|l| l.to_string()).collect();
        assert!(
            text.contains("feat/my-feature"),
            "typed branch name should appear in actions: {text}"
        );
    }

    #[test]
    fn create_task_mode_shows_ctrl_u_action() {
        let theme = Theme::dark();
        let state = state_with_mode(InputMode::CreateTask, ViewMode::Tasks);
        let lines = actions_for_mode(&state, &theme);
        let text: String = lines.iter().map(|l| l.to_string()).collect();
        assert!(
            text.contains("ctrl-u") && text.contains("clear branch name"),
            "create task mode should include ctrl-u clear action: {text}"
        );
    }

    #[test]
    fn scoped_tasks_view_includes_esc_action() {
        let theme = Theme::dark();
        let mut state = state_with_mode(InputMode::Normal, ViewMode::Tasks);
        state.task_repo_scope = Some("github.com/acme/app".to_string());
        let lines = actions_for_mode(&state, &theme);
        let text: String = lines.iter().map(|l| l.to_string()).collect();
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
        let text: String = lines.iter().map(|l| l.to_string()).collect();
        assert!(
            !text.contains("back to repos"),
            "unscoped tasks view should not include back to repos: {text}"
        );
    }
}
