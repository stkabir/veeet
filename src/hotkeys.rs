use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

use crate::config::HotkeyCfg;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotkeyAction {
    ToggleVisible,
    CycleMode,
    ToggleClickThrough,
    ToggleDebug,
    ToggleMenu,
    Quit,
}

pub struct Hotkeys {
    _manager: Option<GlobalHotKeyManager>,
    bindings: Vec<(u32, HotkeyAction)>,
}

impl Hotkeys {
    pub fn new(cfg: &HotkeyCfg) -> Self {
        let manager = match GlobalHotKeyManager::new() {
            Ok(manager) => manager,
            Err(err) => {
                log::error!("No se pudieron registrar hotkeys globales: {err}");
                return Self {
                    _manager: None,
                    bindings: Vec::new(),
                };
            }
        };

        let mut bindings = Vec::new();
        let entries = [
            (cfg.toggle_visible.as_str(), HotkeyAction::ToggleVisible),
            (cfg.cycle_mode.as_str(), HotkeyAction::CycleMode),
            (
                cfg.toggle_click_through.as_str(),
                HotkeyAction::ToggleClickThrough,
            ),
            (cfg.toggle_debug.as_str(), HotkeyAction::ToggleDebug),
            (cfg.toggle_menu.as_str(), HotkeyAction::ToggleMenu),
            (cfg.quit.as_str(), HotkeyAction::Quit),
        ];

        for (text, action) in entries {
            match text.parse::<HotKey>() {
                Ok(hotkey) => {
                    let id = hotkey.id();
                    if let Err(err) = manager.register(hotkey) {
                        log::warn!("No se pudo registrar '{text}': {err}");
                    } else {
                        log::info!("Hotkey registrada: {text} -> {action:?}");
                        bindings.push((id, action));
                    }
                }
                Err(err) => log::warn!("Hotkey inválida '{text}': {err}"),
            }
        }

        Self {
            _manager: Some(manager),
            bindings,
        }
    }

    pub fn poll(&self) -> Vec<HotkeyAction> {
        if self._manager.is_none() {
            return Vec::new();
        }
        let mut actions = Vec::new();
        while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            if event.state == HotKeyState::Pressed
                && let Some((_, action)) = self.bindings.iter().find(|(id, _)| *id == event.id)
            {
                actions.push(*action);
            }
        }
        actions
    }
}
