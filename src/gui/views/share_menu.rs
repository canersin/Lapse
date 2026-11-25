//! "Share" menu used by the trimmer and the clip cards: a public link (Cloudflare
//! tunnel) or the clip sent to a Discord channel through the webhook.

use eframe::egui::{self, RichText};
use std::path::Path;
use std::time::Duration;

use crate::discord::SendState;
use crate::gui::components::dropdown::m3_dropdown_sized;
use crate::gui::i18n::t;
use crate::gui::theme::MonetTheme;

/// Settings > Services, where the Discord webhook is set.
const SERVICES_TAB: usize = 4;

pub enum ShareChoice {
    Link,
    Discord,
    Streamable,
}

/// Webhook from the config file, read at most every few seconds.
pub fn webhook_url() -> String {
    static CACHE: std::sync::Mutex<Option<(std::time::Instant, String)>> = std::sync::Mutex::new(None);
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, url)) = &*c {
        if at.elapsed().as_secs() < 3 {
            return url.clone();
        }
    }
    let url = crate::config::Config::load().map(|c| c.discord_webhook).unwrap_or_default();
    *c = Some((std::time::Instant::now(), url.clone()));
    url
}

/// The menu button; returns what was picked. `too_big_hint` explains what happens to
/// clips over Discord's 10 MB limit.
pub fn share_menu(ui: &mut egui::Ui, theme: &MonetTheme, id: impl std::hash::Hash, bytes: u64, height: f32, too_big_hint: &str) -> Option<ShareChoice> {
    let has_webhook = !webhook_url().is_empty();
    let mut choice = None;
    m3_dropdown_sized(ui, theme, id, t("Share"), 140.0, height, |ui| {
        let hint = if !has_webhook {
            t("Set a webhook in Settings > Services first.")
        } else if bytes > crate::discord::MAX_BYTES {
            too_big_hint
        } else {
            t("Uploads the clip to your Discord channel.")
        };
        if ui.selectable_label(false, t("Send to Discord")).on_hover_text(hint).clicked() {
            if has_webhook {
                choice = Some(ShareChoice::Discord);
            } else {
                crate::gui::components::notice::request_setup(
                    ui.ctx(),
                    t("Discord isn't set up yet"),
                    t("To send clips to a Discord channel, paste the channel's webhook link in Settings > Services > Discord."),
                    SERVICES_TAB,
                );
            }
        }
        let has_streamable = crate::streamable::is_configured();
        let streamable_hint = if !has_streamable {
            t("Configure your Streamable account in Settings > Services first.")
        } else if bytes > crate::streamable::MAX_BYTES {
            t("File exceeds Streamable's 250 MB limit.")
        } else {
            t("Uploads the clip to your Streamable account.")
        };
        if ui.selectable_label(false, t("Upload to Streamable")).on_hover_text(streamable_hint).clicked() {
            if has_streamable {
                choice = Some(ShareChoice::Streamable);
            } else {
                crate::gui::components::notice::request_setup(
                    ui.ctx(),
                    t("Streamable isn't set up yet"),
                    t("To upload clips to Streamable, enter your account email and password in Settings > Services > Streamable."),
                    SERVICES_TAB,
                );
            }
        }
        if ui.selectable_label(false, t("Public link (Cloudflare proxy)")).on_hover_text(t("Streams the clip from your PC through a Cloudflare proxy. Anyone with the link can watch it while Lapse is running.")).clicked() {
            choice = Some(ShareChoice::Link);
        }
    });
    choice
}

/// One line about a Discord upload of `path`, if there is one.
pub fn discord_status(ui: &mut egui::Ui, theme: &MonetTheme, path: &Path) {
    let Some(state) = crate::discord::state_for(path) else { return };
    let (text, color) = match state {
        SendState::Shrinking => (t("Shrinking for Discord...").to_string(), theme.primary),
        SendState::Sending => (t("Sending to Discord...").to_string(), theme.primary),
        SendState::Sent => (t("Sent to Discord").to_string(), theme.success),
        SendState::Failed(e) => (format!("{} {}", t("Discord:"), e), theme.error),
    };
    ui.add(egui::Label::new(RichText::new(text).size(11.5).color(color)).truncate(true));
    ui.ctx().request_repaint_after(Duration::from_millis(300));
}

/// One line about a Streamable upload of `path`, if there is one.
pub fn streamable_status(ui: &mut egui::Ui, theme: &MonetTheme, path: &Path) {
    let Some(state) = crate::streamable::state_for(path) else { return };
    match state {
        crate::streamable::UploadState::Uploading => {
            ui.add(egui::Label::new(RichText::new(t("Uploading to Streamable...")).size(11.5).color(theme.primary)).truncate(true));
            ui.ctx().request_repaint_after(Duration::from_millis(300));
        }
        crate::streamable::UploadState::Done(url) => {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.add(egui::Label::new(RichText::new(&url).size(11.5).color(theme.success)).truncate(true));
                if crate::gui::components::button::m3_small_button(ui, theme, t("Copy")).clicked() {
                    crate::streamable::copy_to_clipboard(&url);
                    crate::utils::send_notification("Lapse", "Streamable link copied", "emblem-shared");
                }
            });
        }
        crate::streamable::UploadState::Failed(e) => {
            ui.add(egui::Label::new(RichText::new(format!("{} {}", t("Streamable:"), e)).size(11.5).color(theme.error)).truncate(true));
            ui.ctx().request_repaint_after(Duration::from_millis(300));
        }
    }
}

