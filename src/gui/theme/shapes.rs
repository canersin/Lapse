use eframe::egui::Rounding;

pub struct M3Shapes;

#[allow(dead_code)]
impl M3Shapes {
    pub const NONE: Rounding = Rounding::ZERO;
    pub const EXTRA_SMALL: Rounding = Rounding { nw: 6.0, ne: 6.0, sw: 6.0, se: 6.0 };
    pub const SMALL: Rounding = Rounding { nw: 10.0, ne: 10.0, sw: 10.0, se: 10.0 };
    pub const MEDIUM: Rounding = Rounding { nw: 16.0, ne: 16.0, sw: 16.0, se: 16.0 };
    pub const LARGE: Rounding = Rounding { nw: 20.0, ne: 20.0, sw: 20.0, se: 20.0 };
    pub const EXTRA_LARGE: Rounding = Rounding { nw: 28.0, ne: 28.0, sw: 28.0, se: 28.0 };
    pub const FULL: Rounding = Rounding { nw: 100.0, ne: 100.0, sw: 100.0, se: 100.0 };
}
