use eframe::egui::{Color32, RichText};

pub struct M3Typography;

#[allow(dead_code)]
impl M3Typography {
    pub fn display_large(text: impl Into<String>, color: Color32) -> RichText {
        RichText::new(text).size(30.0).strong().color(color)
    }

    pub fn headline_large(text: impl Into<String>, color: Color32) -> RichText {
        RichText::new(text).size(24.0).strong().color(color)
    }

    pub fn headline_medium(text: impl Into<String>, color: Color32) -> RichText {
        RichText::new(text).size(20.0).strong().color(color)
    }

    pub fn title_large(text: impl Into<String>, color: Color32) -> RichText {
        RichText::new(text).size(18.0).strong().color(color)
    }

    pub fn title_medium(text: impl Into<String>, color: Color32) -> RichText {
        RichText::new(text).size(15.0).strong().color(color)
    }

    pub fn body_large(text: impl Into<String>, color: Color32) -> RichText {
        RichText::new(text).size(14.0).color(color)
    }

    pub fn body_medium(text: impl Into<String>, color: Color32) -> RichText {
        RichText::new(text).size(13.0).color(color)
    }

    pub fn label_large(text: impl Into<String>, color: Color32) -> RichText {
        RichText::new(text).size(13.0).strong().color(color)
    }

    pub fn label_medium(text: impl Into<String>, color: Color32) -> RichText {
        RichText::new(text).size(11.0).strong().color(color)
    }

    pub fn label_small(text: impl Into<String>, color: Color32) -> RichText {
        RichText::new(text).size(10.0).color(color)
    }
}
