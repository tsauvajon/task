use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::state::InputMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum UiIntent {
    Quit,
    SwitchView,
    MoveNext,
    MovePrev,
    ToggleHelp,
    OpenSelected,
    EnterFilterMode,
    EnterCreateTaskMode,
    FinishSelected,
    RefreshCurrentView,
    ParkSelected,
    FilterCancel,
    FilterApply,
    FilterBackspace,
    FilterClear,
    FilterAppend(char),
    CreateCancel,
    CreateSubmit,
    CreateBackspace,
    CreateAppend(char),
    CloneCancel,
    CloneSubmit,
    CloneBackspace,
    CloneClear,
    CloneAppend(char),
    Noop,
}

pub(super) fn from_key(mode: InputMode, key: KeyEvent) -> UiIntent {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return UiIntent::Quit;
    }

    match mode {
        InputMode::Normal => from_key_normal(key),
        InputMode::Filter => from_key_filter(key),
        InputMode::CreateTask => from_key_create(key),
        InputMode::CloneRepo => from_key_clone(key),
    }
}

fn from_key_normal(key: KeyEvent) -> UiIntent {
    match key.code {
        KeyCode::Char('q') => UiIntent::Quit,
        KeyCode::Tab => UiIntent::SwitchView,
        KeyCode::Down | KeyCode::Char('j') => UiIntent::MoveNext,
        KeyCode::Up | KeyCode::Char('k') => UiIntent::MovePrev,
        KeyCode::Char('/') => UiIntent::EnterFilterMode,
        KeyCode::Char('?') => UiIntent::ToggleHelp,
        KeyCode::Char('c') => UiIntent::EnterCreateTaskMode,
        KeyCode::Char('f') => UiIntent::FinishSelected,
        KeyCode::Char('r') => UiIntent::RefreshCurrentView,
        KeyCode::Char('p') => UiIntent::ParkSelected,
        KeyCode::Enter => UiIntent::OpenSelected,
        _ => UiIntent::Noop,
    }
}

fn from_key_filter(key: KeyEvent) -> UiIntent {
    match key.code {
        KeyCode::Esc => UiIntent::FilterCancel,
        KeyCode::Enter => UiIntent::FilterApply,
        KeyCode::Backspace => UiIntent::FilterBackspace,
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            UiIntent::FilterClear
        }
        KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            UiIntent::FilterAppend(ch)
        }
        _ => UiIntent::Noop,
    }
}

fn from_key_create(key: KeyEvent) -> UiIntent {
    match key.code {
        KeyCode::Esc => UiIntent::CreateCancel,
        KeyCode::Enter => UiIntent::CreateSubmit,
        KeyCode::Backspace => UiIntent::CreateBackspace,
        KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            UiIntent::CreateAppend(ch)
        }
        _ => UiIntent::Noop,
    }
}

fn from_key_clone(key: KeyEvent) -> UiIntent {
    match key.code {
        KeyCode::Esc => UiIntent::CloneCancel,
        KeyCode::Enter => UiIntent::CloneSubmit,
        KeyCode::Backspace => UiIntent::CloneBackspace,
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => UiIntent::CloneClear,
        KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            UiIntent::CloneAppend(ch)
        }
        _ => UiIntent::Noop,
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::{UiIntent, from_key};
    use crate::ui::state::InputMode;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn normal_mode_maps_navigation_intents() {
        assert_eq!(
            from_key(InputMode::Normal, key(KeyCode::Down)),
            UiIntent::MoveNext
        );
        assert_eq!(
            from_key(InputMode::Normal, key(KeyCode::Char('k'))),
            UiIntent::MovePrev
        );
        assert_eq!(
            from_key(InputMode::Normal, key(KeyCode::Enter)),
            UiIntent::OpenSelected
        );
    }

    #[test]
    fn filter_mode_maps_editing_intents() {
        assert_eq!(
            from_key(InputMode::Filter, key(KeyCode::Esc)),
            UiIntent::FilterCancel
        );
        assert_eq!(
            from_key(InputMode::Filter, key(KeyCode::Backspace)),
            UiIntent::FilterBackspace
        );
        assert_eq!(
            from_key(InputMode::Filter, key(KeyCode::Char('a'))),
            UiIntent::FilterAppend('a')
        );
    }

    #[test]
    fn create_mode_maps_create_intents() {
        assert_eq!(
            from_key(InputMode::CreateTask, key(KeyCode::Esc)),
            UiIntent::CreateCancel
        );
        assert_eq!(
            from_key(InputMode::CreateTask, key(KeyCode::Enter)),
            UiIntent::CreateSubmit
        );
        assert_eq!(
            from_key(InputMode::CreateTask, key(KeyCode::Char('b'))),
            UiIntent::CreateAppend('b')
        );
    }

    #[test]
    fn normal_mode_maps_view_switch_intent() {
        assert_eq!(
            from_key(InputMode::Normal, key(KeyCode::Tab)),
            UiIntent::SwitchView
        );
    }

    #[test]
    fn clone_mode_maps_clone_intents() {
        assert_eq!(
            from_key(InputMode::CloneRepo, key(KeyCode::Esc)),
            UiIntent::CloneCancel
        );
        assert_eq!(
            from_key(InputMode::CloneRepo, key(KeyCode::Enter)),
            UiIntent::CloneSubmit
        );
        assert_eq!(
            from_key(InputMode::CloneRepo, key(KeyCode::Char('g'))),
            UiIntent::CloneAppend('g')
        );
    }

    #[test]
    fn ctrl_c_quits_in_any_mode() {
        let key = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(from_key(InputMode::Normal, key), UiIntent::Quit);
        assert_eq!(from_key(InputMode::Filter, key), UiIntent::Quit);
        assert_eq!(from_key(InputMode::CreateTask, key), UiIntent::Quit);
        assert_eq!(from_key(InputMode::CloneRepo, key), UiIntent::Quit);
    }
}
