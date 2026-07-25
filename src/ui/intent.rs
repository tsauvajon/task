use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::state::InputMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum UiIntent {
    Quit,
    SwitchView,
    MoveNext,
    MovePrev,
    PageDown,
    PageUp,
    HalfPageDown,
    HalfPageUp,
    MoveFirst,
    MoveLast,
    ToggleHelp,
    OpenSelected,
    EnterFilterMode,
    EnterCreateTaskMode,
    EnterCloneMode,
    FinishSelected,
    RefreshCurrentView,
    ParkSelected,
    ToggleDetach,
    ToggleSidebar,
    ClearScope,
    ClickTaskRow(usize),
    ClickRepoRow(usize),
    FilterCancel,
    FilterApply,
    FilterBackspace,
    InputStart,
    InputEnd,
    InputKillBackward,
    InputKillForward,
    FilterAppend(char),
    CreateCancel,
    CreateSubmit,
    CreateBackspace,
    CreateAppend(char),
    CloneCancel,
    CloneSubmit,
    CloneBackspace,
    CloneAppend(char),
    UnboundKey,
    Noop,
}

pub(super) fn from_key(mode: InputMode, key: KeyEvent) -> UiIntent {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return UiIntent::Quit;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('p') {
        return UiIntent::ToggleHelp;
    }

    match mode {
        InputMode::Normal => from_key_normal(key),
        InputMode::Filter => from_key_filter(key),
        InputMode::CreateTask => from_key_create(key),
        InputMode::CloneRepo => from_key_clone(key),
    }
}

const fn from_key_normal(key: KeyEvent) -> UiIntent {
    match key.code {
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            UiIntent::HalfPageDown
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => UiIntent::HalfPageUp,
        KeyCode::Char('q') => UiIntent::Quit,
        KeyCode::Tab => UiIntent::SwitchView,
        KeyCode::Down | KeyCode::Char('j') => UiIntent::MoveNext,
        KeyCode::Up | KeyCode::Char('k') => UiIntent::MovePrev,
        KeyCode::PageDown => UiIntent::PageDown,
        KeyCode::PageUp => UiIntent::PageUp,
        KeyCode::Home => UiIntent::MoveFirst,
        KeyCode::End => UiIntent::MoveLast,
        KeyCode::Char('/') => UiIntent::EnterFilterMode,
        KeyCode::Char('t') => UiIntent::EnterCreateTaskMode,
        KeyCode::Char('b') => UiIntent::ToggleSidebar,
        KeyCode::Char('c') => UiIntent::EnterCloneMode,
        KeyCode::Char('d') => UiIntent::ToggleDetach,
        KeyCode::Char('f') => UiIntent::FinishSelected,
        KeyCode::Char('r') => UiIntent::RefreshCurrentView,
        KeyCode::Char('p') => UiIntent::ParkSelected,
        KeyCode::Enter => UiIntent::OpenSelected,
        KeyCode::Esc => UiIntent::ClearScope,
        KeyCode::Backspace
        | KeyCode::Left
        | KeyCode::Right
        | KeyCode::BackTab
        | KeyCode::Delete
        | KeyCode::Insert
        | KeyCode::F(_)
        | KeyCode::Char(_)
        | KeyCode::Null
        | KeyCode::CapsLock
        | KeyCode::ScrollLock
        | KeyCode::NumLock
        | KeyCode::PrintScreen
        | KeyCode::Pause
        | KeyCode::Menu
        | KeyCode::KeypadBegin
        | KeyCode::Media(_)
        | KeyCode::Modifier(_) => UiIntent::UnboundKey,
    }
}

const fn from_key_filter(key: KeyEvent) -> UiIntent {
    match key.code {
        KeyCode::Tab => UiIntent::SwitchView,
        KeyCode::Esc => UiIntent::FilterCancel,
        KeyCode::Enter => UiIntent::FilterApply,
        KeyCode::Backspace => UiIntent::FilterBackspace,
        KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => UiIntent::InputStart,
        KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => UiIntent::InputEnd,
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            UiIntent::InputKillBackward
        }
        KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            UiIntent::InputKillForward
        }
        KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            UiIntent::FilterAppend(ch)
        }
        KeyCode::Left
        | KeyCode::Right
        | KeyCode::Up
        | KeyCode::Down
        | KeyCode::Home
        | KeyCode::End
        | KeyCode::PageUp
        | KeyCode::PageDown
        | KeyCode::BackTab
        | KeyCode::Delete
        | KeyCode::Insert
        | KeyCode::F(_)
        | KeyCode::Char(_)
        | KeyCode::Null
        | KeyCode::CapsLock
        | KeyCode::ScrollLock
        | KeyCode::NumLock
        | KeyCode::PrintScreen
        | KeyCode::Pause
        | KeyCode::Menu
        | KeyCode::KeypadBegin
        | KeyCode::Media(_)
        | KeyCode::Modifier(_) => UiIntent::UnboundKey,
    }
}

const fn from_key_create(key: KeyEvent) -> UiIntent {
    match key.code {
        KeyCode::Esc => UiIntent::CreateCancel,
        KeyCode::Enter => UiIntent::CreateSubmit,
        KeyCode::Backspace => UiIntent::CreateBackspace,
        KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => UiIntent::InputStart,
        KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => UiIntent::InputEnd,
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            UiIntent::InputKillBackward
        }
        KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            UiIntent::InputKillForward
        }
        KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            UiIntent::CreateAppend(ch)
        }
        KeyCode::Left
        | KeyCode::Right
        | KeyCode::Up
        | KeyCode::Down
        | KeyCode::Home
        | KeyCode::End
        | KeyCode::PageUp
        | KeyCode::PageDown
        | KeyCode::Tab
        | KeyCode::BackTab
        | KeyCode::Delete
        | KeyCode::Insert
        | KeyCode::F(_)
        | KeyCode::Char(_)
        | KeyCode::Null
        | KeyCode::CapsLock
        | KeyCode::ScrollLock
        | KeyCode::NumLock
        | KeyCode::PrintScreen
        | KeyCode::Pause
        | KeyCode::Menu
        | KeyCode::KeypadBegin
        | KeyCode::Media(_)
        | KeyCode::Modifier(_) => UiIntent::UnboundKey,
    }
}

const fn from_key_clone(key: KeyEvent) -> UiIntent {
    match key.code {
        KeyCode::Esc => UiIntent::CloneCancel,
        KeyCode::Enter => UiIntent::CloneSubmit,
        KeyCode::Backspace => UiIntent::CloneBackspace,
        KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => UiIntent::InputStart,
        KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => UiIntent::InputEnd,
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            UiIntent::InputKillBackward
        }
        KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            UiIntent::InputKillForward
        }
        KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            UiIntent::CloneAppend(ch)
        }
        KeyCode::Left
        | KeyCode::Right
        | KeyCode::Up
        | KeyCode::Down
        | KeyCode::Home
        | KeyCode::End
        | KeyCode::PageUp
        | KeyCode::PageDown
        | KeyCode::Tab
        | KeyCode::BackTab
        | KeyCode::Delete
        | KeyCode::Insert
        | KeyCode::F(_)
        | KeyCode::Char(_)
        | KeyCode::Null
        | KeyCode::CapsLock
        | KeyCode::ScrollLock
        | KeyCode::NumLock
        | KeyCode::PrintScreen
        | KeyCode::Pause
        | KeyCode::Menu
        | KeyCode::KeypadBegin
        | KeyCode::Media(_)
        | KeyCode::Modifier(_) => UiIntent::UnboundKey,
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

    mod normal_mode {
        use super::*;

        #[test]
        fn maps_navigation_intents() {
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
        fn maps_view_switch_intent() {
            assert_eq!(
                from_key(InputMode::Normal, key(KeyCode::Tab)),
                UiIntent::SwitchView
            );
        }

        #[test]
        fn maps_all_normal_keys() {
            assert_eq!(
                from_key(InputMode::Normal, key(KeyCode::Char('q'))),
                UiIntent::Quit
            );
            assert_eq!(
                from_key(InputMode::Normal, key(KeyCode::Up)),
                UiIntent::MovePrev
            );
            assert_eq!(
                from_key(InputMode::Normal, key(KeyCode::Char('j'))),
                UiIntent::MoveNext
            );
            assert_eq!(
                from_key(InputMode::Normal, key(KeyCode::Char('/'))),
                UiIntent::EnterFilterMode
            );
            assert_eq!(
                from_key(InputMode::Normal, key(KeyCode::Char('t'))),
                UiIntent::EnterCreateTaskMode
            );
            assert_eq!(
                from_key(InputMode::Normal, key(KeyCode::Char('c'))),
                UiIntent::EnterCloneMode
            );
            assert_eq!(
                from_key(InputMode::Normal, key(KeyCode::Char('d'))),
                UiIntent::ToggleDetach
            );
            assert_eq!(
                from_key(InputMode::Normal, key(KeyCode::Char('f'))),
                UiIntent::FinishSelected
            );
            assert_eq!(
                from_key(InputMode::Normal, key(KeyCode::Char('r'))),
                UiIntent::RefreshCurrentView
            );
            assert_eq!(
                from_key(InputMode::Normal, key(KeyCode::Char('p'))),
                UiIntent::ParkSelected
            );
            assert_eq!(
                from_key(InputMode::Normal, key(KeyCode::Char('b'))),
                UiIntent::ToggleSidebar
            );
        }

        #[test]
        fn b_is_append_not_toggle_in_editing_modes() {
            // Regression guard: the sidebar toggle must not hijack `b`
            // in filter / create / clone modes where `b` is just text.
            assert_eq!(
                from_key(InputMode::Filter, key(KeyCode::Char('b'))),
                UiIntent::FilterAppend('b')
            );
            assert_eq!(
                from_key(InputMode::CreateTask, key(KeyCode::Char('b'))),
                UiIntent::CreateAppend('b')
            );
            assert_eq!(
                from_key(InputMode::CloneRepo, key(KeyCode::Char('b'))),
                UiIntent::CloneAppend('b')
            );
        }

        #[test]
        fn maps_page_and_home_end_keys() {
            assert_eq!(
                from_key(InputMode::Normal, key(KeyCode::PageDown)),
                UiIntent::PageDown
            );
            assert_eq!(
                from_key(InputMode::Normal, key(KeyCode::PageUp)),
                UiIntent::PageUp
            );
            assert_eq!(
                from_key(InputMode::Normal, key(KeyCode::Home)),
                UiIntent::MoveFirst
            );
            assert_eq!(
                from_key(InputMode::Normal, key(KeyCode::End)),
                UiIntent::MoveLast
            );
        }

        #[test]
        fn ctrl_u_and_ctrl_d_map_to_half_page_navigation() {
            let ctrl_u = KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL);
            let ctrl_d = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL);
            assert_eq!(from_key(InputMode::Normal, ctrl_u), UiIntent::HalfPageUp);
            assert_eq!(from_key(InputMode::Normal, ctrl_d), UiIntent::HalfPageDown);
        }

        #[test]
        fn ctrl_k_keeps_previous_row_navigation() {
            let ctrl_k = KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL);
            assert_eq!(from_key(InputMode::Normal, ctrl_k), UiIntent::MovePrev);
        }

        #[test]
        fn unhandled_ctrl_letter_keeps_existing_plain_key_behavior() {
            let ctrl_t = KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL);
            assert_eq!(
                from_key(InputMode::Normal, ctrl_t),
                UiIntent::EnterCreateTaskMode
            );
        }

        #[test]
        fn esc_maps_to_clear_scope() {
            assert_eq!(
                from_key(InputMode::Normal, key(KeyCode::Esc)),
                UiIntent::ClearScope
            );
        }

        #[test]
        fn question_mark_is_noop_after_ctrl_p_migration() {
            assert_eq!(
                from_key(InputMode::Normal, key(KeyCode::Char('?'))),
                UiIntent::UnboundKey
            );
        }

        #[test]
        fn unrecognised_key_maps_to_noop() {
            assert_eq!(
                from_key(InputMode::Normal, key(KeyCode::F(1))),
                UiIntent::UnboundKey
            );
        }
    }

    mod filter_mode {
        use super::*;

        #[test]
        fn maps_editing_intents() {
            assert_eq!(
                from_key(InputMode::Filter, key(KeyCode::Tab)),
                UiIntent::SwitchView
            );
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
        fn enter_maps_to_filter_apply() {
            assert_eq!(
                from_key(InputMode::Filter, key(KeyCode::Enter)),
                UiIntent::FilterApply
            );
        }

        #[test]
        fn ctrl_u_and_ctrl_k_map_to_kill_intents() {
            let ctrl_u = KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL);
            let ctrl_k = KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL);
            assert_eq!(
                from_key(InputMode::Filter, ctrl_u),
                UiIntent::InputKillBackward
            );
            assert_eq!(
                from_key(InputMode::Filter, ctrl_k),
                UiIntent::InputKillForward
            );
        }

        #[test]
        fn ctrl_a_and_ctrl_e_move_filter_cursor() {
            let ctrl_a = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL);
            let ctrl_e = KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL);
            assert_eq!(from_key(InputMode::Filter, ctrl_a), UiIntent::InputStart);
            assert_eq!(from_key(InputMode::Filter, ctrl_e), UiIntent::InputEnd);
        }

        #[test]
        fn unrecognised_key_maps_to_noop() {
            assert_eq!(
                from_key(InputMode::Filter, key(KeyCode::F(1))),
                UiIntent::UnboundKey
            );
        }
    }

    mod create_mode {
        use super::*;

        #[test]
        fn maps_create_intents() {
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
        fn backspace_maps_to_create_backspace() {
            assert_eq!(
                from_key(InputMode::CreateTask, key(KeyCode::Backspace)),
                UiIntent::CreateBackspace
            );
        }

        #[test]
        fn ctrl_u_and_ctrl_k_map_to_kill_intents() {
            let ctrl_u = KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL);
            let ctrl_k = KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL);
            assert_eq!(
                from_key(InputMode::CreateTask, ctrl_u),
                UiIntent::InputKillBackward
            );
            assert_eq!(
                from_key(InputMode::CreateTask, ctrl_k),
                UiIntent::InputKillForward
            );
        }

        #[test]
        fn ctrl_a_and_ctrl_e_move_create_cursor() {
            let ctrl_a = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL);
            let ctrl_e = KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL);
            assert_eq!(
                from_key(InputMode::CreateTask, ctrl_a),
                UiIntent::InputStart
            );
            assert_eq!(from_key(InputMode::CreateTask, ctrl_e), UiIntent::InputEnd);
        }
    }

    mod clone_mode {
        use super::*;

        #[test]
        fn maps_clone_intents() {
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
        fn backspace_maps_to_clone_backspace() {
            assert_eq!(
                from_key(InputMode::CloneRepo, key(KeyCode::Backspace)),
                UiIntent::CloneBackspace
            );
        }

        #[test]
        fn ctrl_u_and_ctrl_k_map_to_kill_intents() {
            let ctrl_u = KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL);
            let ctrl_k = KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL);
            assert_eq!(
                from_key(InputMode::CloneRepo, ctrl_u),
                UiIntent::InputKillBackward
            );
            assert_eq!(
                from_key(InputMode::CloneRepo, ctrl_k),
                UiIntent::InputKillForward
            );
        }

        #[test]
        fn ctrl_a_and_ctrl_e_move_clone_cursor() {
            let ctrl_a = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL);
            let ctrl_e = KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL);
            assert_eq!(from_key(InputMode::CloneRepo, ctrl_a), UiIntent::InputStart);
            assert_eq!(from_key(InputMode::CloneRepo, ctrl_e), UiIntent::InputEnd);
        }

        #[test]
        fn unrecognised_key_maps_to_noop() {
            assert_eq!(
                from_key(InputMode::CloneRepo, key(KeyCode::F(5))),
                UiIntent::UnboundKey
            );
        }
    }

    mod global {
        use super::*;

        #[test]
        fn ctrl_c_quits_in_any_mode() {
            let key = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
            assert_eq!(from_key(InputMode::Normal, key), UiIntent::Quit);
            assert_eq!(from_key(InputMode::Filter, key), UiIntent::Quit);
            assert_eq!(from_key(InputMode::CreateTask, key), UiIntent::Quit);
            assert_eq!(from_key(InputMode::CloneRepo, key), UiIntent::Quit);
        }

        #[test]
        fn ctrl_p_toggles_help_in_any_mode() {
            let key = KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL);
            assert_eq!(from_key(InputMode::Normal, key), UiIntent::ToggleHelp);
            assert_eq!(from_key(InputMode::Filter, key), UiIntent::ToggleHelp);
            assert_eq!(from_key(InputMode::CreateTask, key), UiIntent::ToggleHelp);
            assert_eq!(from_key(InputMode::CloneRepo, key), UiIntent::ToggleHelp);
        }
    }
}
