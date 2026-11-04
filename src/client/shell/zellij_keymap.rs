//! Local modal input policy. Actions use the existing server API and dialogs.
use super::*;
use crate::input::{KeybindAction as Action, KeybindMatch, TerminalKey};
use crossterm::event::KeyModifiers;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mode {
    Locked,
    Pane,
    Tab,
    Resize,
    Move,
    Session,
}

impl Mode {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Locked => "LOCKED",
            Self::Pane => "PANE",
            Self::Tab => "TAB",
            Self::Resize => "RESIZE",
            Self::Move => "MOVE",
            Self::Session => "SESSION",
        }
    }

    pub(super) fn hint(self) -> &'static str {
        match self {
            Self::Locked => "Ctrl+g unlock; all other keys go to the pane",
            Self::Pane => "hjkl/arrows focus  n/r split right  d split down  x close  f zoom  c rename  Esc done",
            Self::Tab => "h/l/arrows switch  1-9 select  n new  x close  r rename  Esc done",
            Self::Resize => "hjkl/arrows resize  +/- grow/shrink width  Esc done",
            Self::Move => "hjkl/arrows swap panes  n/p cycle swap  Esc done",
            Self::Session => "w workspaces  n new workspace  r rename  d detach  c settings  Esc done",
        }
    }
}

fn direction(code: KeyCode) -> Option<usize> {
    match code {
        KeyCode::Char('h') | KeyCode::Left => Some(0),
        KeyCode::Char('j') | KeyCode::Down => Some(1),
        KeyCode::Char('k') | KeyCode::Up => Some(2),
        KeyCode::Char('l') | KeyCode::Right => Some(3),
        _ => None,
    }
}

fn mode_action(mode: Mode, code: KeyCode) -> Option<(Action, bool)> {
    // The boolean returns to normal before an action opens a dialog or changes context.
    if let Some(index) = direction(code)
        .filter(|_| matches!(mode, Mode::Pane | Mode::Resize | Mode::Move | Mode::Tab))
    {
        let actions = match mode {
            Mode::Pane => [
                Action::FocusPaneLeft,
                Action::FocusPaneDown,
                Action::FocusPaneUp,
                Action::FocusPaneRight,
            ],
            Mode::Resize => [
                Action::ResizePaneLeft,
                Action::ResizePaneDown,
                Action::ResizePaneUp,
                Action::ResizePaneRight,
            ],
            Mode::Move => [
                Action::SwapPaneLeft,
                Action::SwapPaneDown,
                Action::SwapPaneUp,
                Action::SwapPaneRight,
            ],
            Mode::Tab => [
                Action::PreviousTab,
                Action::NextTab,
                Action::PreviousTab,
                Action::NextTab,
            ],
            _ => return None,
        };
        return Some((actions[index], false));
    }
    let action = match (mode, code) {
        (Mode::Pane, KeyCode::Char('n' | 'r')) => Action::SplitVertical,
        (Mode::Pane, KeyCode::Char('d')) => Action::SplitHorizontal,
        (Mode::Pane, KeyCode::Char('x')) => Action::ClosePane,
        (Mode::Pane, KeyCode::Char('f')) => Action::Zoom,
        (Mode::Pane, KeyCode::Char('c')) => Action::RenamePane,
        (Mode::Pane, KeyCode::Char('w')) => Action::ToggleSidebar,
        (Mode::Pane, KeyCode::Char('p')) => Action::CyclePaneNext,
        (Mode::Tab, KeyCode::Char('n')) => Action::NewTab,
        (Mode::Tab, KeyCode::Char('x')) => Action::CloseTab,
        (Mode::Tab, KeyCode::Char('r')) => Action::RenameTab,
        (Mode::Tab, KeyCode::Char(c @ '1'..='9')) => Action::SwitchTab((c as u8 - b'1') as usize),
        (Mode::Tab, KeyCode::Char('[')) => Action::MoveTabPrevious,
        (Mode::Tab, KeyCode::Char(']')) => Action::MoveTabNext,
        (Mode::Move, KeyCode::Char('n')) => return Some((Action::SwapPaneRight, false)),
        (Mode::Move, KeyCode::Char('p')) => return Some((Action::SwapPaneLeft, false)),
        (Mode::Resize, KeyCode::Char('+' | '=')) => return Some((Action::ResizePaneRight, false)),
        (Mode::Resize, KeyCode::Char('-')) => return Some((Action::ResizePaneLeft, false)),
        (Mode::Session, KeyCode::Char('d')) => Action::Detach,
        (Mode::Session, KeyCode::Char('w')) => Action::WorkspacePicker,
        (Mode::Session, KeyCode::Char('n')) => Action::NewWorkspace,
        (Mode::Session, KeyCode::Char('r')) => Action::RenameWorkspace,
        (Mode::Session, KeyCode::Char('c')) => Action::Settings,
        (Mode::Session, KeyCode::Char('l')) => Action::ReloadConfig,
        (_, KeyCode::Char('?')) => Action::Help,
        _ => return None,
    };
    Some((action, true))
}

impl ClientShellState {
    pub(super) fn route_zellij_key(
        &mut self,
        key: &TerminalKey,
        outcome: &mut ClientShellInput,
    ) -> bool {
        if !self.config.zellij_keymap {
            if matches!(self.mode, ClientShellMode::Zellij(_)) {
                self.mode = ClientShellMode::Terminal;
                outcome.repaint = true;
            }
            return false;
        }
        if !matches!(
            self.mode,
            ClientShellMode::Terminal | ClientShellMode::Zellij(_) | ClientShellMode::Copy
        ) || self
            .copy_mode
            .as_ref()
            .is_some_and(|copy| copy.search_prompt.is_some())
        {
            return false;
        }
        let locked = self.mode == ClientShellMode::Zellij(Mode::Locked);
        if key.modifiers == KeyModifiers::CONTROL {
            // A held mode key must not repeatedly toggle the mode, including
            // hosts that send a repeat after the original press was released.
            if key.kind == crossterm::event::KeyEventKind::Repeat
                && (!locked || key.code == KeyCode::Char('g'))
                && matches!(
                    key.code,
                    KeyCode::Char('g' | 'p' | 't' | 'r' | 'h' | 'o' | 's' | 'q')
                )
            {
                return true;
            }
            let next = match key.code {
                KeyCode::Char('g') => Some(Mode::Locked),
                _ if locked => return false,
                KeyCode::Char('p') => Some(Mode::Pane),
                KeyCode::Char('t') => Some(Mode::Tab),
                KeyCode::Char('r') => Some(Mode::Resize),
                KeyCode::Char('h') => Some(Mode::Move),
                KeyCode::Char('o') => Some(Mode::Session),
                KeyCode::Char('s') => {
                    if self.mode == ClientShellMode::Copy {
                        self.exit_copy_mode(false, outcome);
                    } else {
                        self.mode = ClientShellMode::Terminal;
                        self.record_binding(KeybindMatch::Action(Action::CopyMode), outcome);
                    }
                    outcome.repaint = true;
                    return true;
                }
                KeyCode::Char('q') => {
                    if self.mode == ClientShellMode::Copy {
                        self.exit_copy_mode(false, outcome);
                    }
                    self.mode = ClientShellMode::Terminal;
                    self.record_binding(KeybindMatch::Action(Action::Detach), outcome);
                    return true;
                }
                _ => None,
            };
            if let Some(next) = next {
                if self.mode == ClientShellMode::Copy {
                    self.exit_copy_mode(false, outcome);
                }
                self.mode = if self.mode == ClientShellMode::Zellij(next) {
                    ClientShellMode::Terminal
                } else {
                    ClientShellMode::Zellij(next)
                };
                outcome.repaint = true;
                return true;
            }
        }
        if locked || self.mode == ClientShellMode::Copy {
            return false;
        }
        if key.code == KeyCode::F(1) && key.modifiers.is_empty() {
            self.record_binding(KeybindMatch::Action(Action::Help), outcome);
            return true;
        }
        if key.modifiers == KeyModifiers::ALT
            || (key.modifiers == (KeyModifiers::ALT | KeyModifiers::SHIFT)
                && matches!(key.code, KeyCode::Char('+' | '=')))
        {
            let action = direction(key.code)
                .map(|index| {
                    [
                        Action::FocusPaneLeft,
                        Action::FocusPaneDown,
                        Action::FocusPaneUp,
                        Action::FocusPaneRight,
                    ][index]
                })
                .or(match key.code {
                    KeyCode::Char('n') => Some(Action::SplitVertical),
                    KeyCode::Char('f') => Some(Action::Zoom),
                    KeyCode::Char('[') => Some(Action::PreviousTab),
                    KeyCode::Char(']') => Some(Action::NextTab),
                    KeyCode::Char('+' | '=') => Some(Action::ResizePaneRight),
                    KeyCode::Char('-') => Some(Action::ResizePaneLeft),
                    _ => None,
                });
            if let Some(action) = action {
                self.record_binding(KeybindMatch::Action(action), outcome);
                return true;
            }
        }
        let ClientShellMode::Zellij(mode) = self.mode else {
            return false;
        };
        if matches!(key.code, KeyCode::Esc | KeyCode::Enter | KeyCode::Char(' '))
            && key.modifiers.is_empty()
        {
            self.mode = ClientShellMode::Terminal;
            outcome.repaint = true;
        } else if key.modifiers.difference(KeyModifiers::SHIFT).is_empty() {
            let code = crate::input::keybind_help_text_char(key)
                .map(KeyCode::Char)
                .unwrap_or(key.code);
            if let Some((action, leave)) = mode_action(mode, code) {
                if leave {
                    self.mode = ClientShellMode::Terminal;
                }
                self.record_binding(KeybindMatch::Action(action), outcome);
                outcome.repaint = true;
            }
        }
        // Unknown command-mode keys are consumed; they never leak into the shell.
        true
    }
}

pub(super) fn help_groups() -> Vec<crate::input::KeybindHelpGroup> {
    use std::borrow::Cow;
    let entries = [
        ("Ctrl+t", "tab mode: h/l or arrows switch; n new; x close; r rename; 1-9 select"),
        ("Ctrl+p", "pane mode: hjkl/arrows focus; n/r split right; d split down; x close; f zoom; c rename"),
        ("Ctrl+r", "resize mode: hjkl/arrows resize; +/- change width"),
        ("Ctrl+h", "move mode: hjkl/arrows swap panes; n/p swap right/left"),
        ("Ctrl+s", "scroll/copy mode: hjkl move; / search; v select; y copy; q exit"),
        ("Ctrl+o", "session mode: w workspaces; n new workspace; r rename; d detach; c settings; l reload"),
        ("Ctrl+g", "lock/unlock: forward every other key to the pane"),
        ("Alt+h/j/k/l", "focus left/down/up/right (also Alt+arrows)"),
        ("Alt+n / Alt+f", "split right / toggle zoom"),
        ("Alt+[ / Alt+]", "previous / next tab"),
        ("Alt++ / Alt+-", "grow / shrink pane width"),
        ("Ctrl+q", "detach; keep the session running"),
        ("Esc / Enter / Space", "leave a command mode; repeat its Ctrl key to toggle it off"),
        ("F1 / ? in a mode", "show these bindings"),
    ];
    vec![(
        "Zellij-style keymap",
        entries
            .into_iter()
            .map(|(key, label)| (key.to_owned(), Cow::Borrowed(label)))
            .collect(),
    )]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_letter_actions_are_not_shadowed_by_direction_keys() {
        assert_eq!(
            mode_action(Mode::Session, KeyCode::Char('l')),
            Some((Action::ReloadConfig, true))
        );
        assert_eq!(
            mode_action(Mode::Session, KeyCode::Char('n')),
            Some((Action::NewWorkspace, true))
        );
        assert_eq!(
            mode_action(Mode::Session, KeyCode::Char('d')),
            Some((Action::Detach, true))
        );
    }
}
