use std::path::PathBuf;
use std::time::{Duration, Instant};
use crate::gui::theme::MonetPaletteKind;

#[derive(PartialEq, Eq, Clone, Copy, Debug, Hash)]
pub enum NavigationTab {
    Home,
    Library,
    Settings,
    About,
}

pub struct GuiState {
    pub active_tab: NavigationTab,
    pub search_query: String,
    pub selected_game_filter: Option<String>,
    pub status_banner: Option<(String, Instant)>,
    #[allow(dead_code)]
    pub selected_monet_palette: MonetPaletteKind,
    #[allow(dead_code)]
    pub clip_to_delete: Option<PathBuf>,
    #[allow(dead_code)]
    pub editor_target: Option<PathBuf>,
    pub mic_enabled: bool,
    pub desktop_audio_enabled: bool,
}

impl Default for GuiState {
    fn default() -> Self {
        Self {
            active_tab: NavigationTab::Home,
            search_query: String::new(),
            selected_game_filter: None,
            status_banner: None,
            selected_monet_palette: MonetPaletteKind::Lavender,
            clip_to_delete: None,
            editor_target: None,
            mic_enabled: true,
            desktop_audio_enabled: true,
        }
    }
}

#[allow(dead_code)]
impl GuiState {
    pub fn set_banner(&mut self, message: impl Into<String>) {
        self.status_banner = Some((message.into(), Instant::now()));
    }

    pub fn current_banner(&self) -> Option<&str> {
        if let Some((msg, time)) = &self.status_banner {
            if time.elapsed() < Duration::from_secs(4) {
                return Some(msg.as_str());
            }
        }
        None
    }
}
