use eframe::egui::Color32;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MonetPaletteKind {
    Midnight,
    Lavender,
    Ocean,
    Mint,
    Coral,
    Charcoal,
    Sakura,
    Cyberpunk,
    Emerald,
    Crimson,
    Nord,
    TokyoNight,
    Dracula,
    Gruvbox,
    Catppuccin,
    Monokai,
    Vaporwave,
    Amber,
    Teal,
    RosePine,
    Solarized,
    Everforest,
    Kanagawa,
    Ayu,
}

#[allow(dead_code)]
impl MonetPaletteKind {
    pub const ALL: [MonetPaletteKind; 24] = [
        MonetPaletteKind::Midnight,
        MonetPaletteKind::Lavender,
        MonetPaletteKind::Ocean,
        MonetPaletteKind::Mint,
        MonetPaletteKind::Coral,
        MonetPaletteKind::Charcoal,
        MonetPaletteKind::Sakura,
        MonetPaletteKind::Cyberpunk,
        MonetPaletteKind::Emerald,
        MonetPaletteKind::Crimson,
        MonetPaletteKind::Nord,
        MonetPaletteKind::TokyoNight,
        MonetPaletteKind::Dracula,
        MonetPaletteKind::Gruvbox,
        MonetPaletteKind::Catppuccin,
        MonetPaletteKind::Monokai,
        MonetPaletteKind::Vaporwave,
        MonetPaletteKind::Amber,
        MonetPaletteKind::Teal,
        MonetPaletteKind::RosePine,
        MonetPaletteKind::Solarized,
        MonetPaletteKind::Everforest,
        MonetPaletteKind::Kanagawa,
        MonetPaletteKind::Ayu,
    ];

    pub fn from_config_name(name: &str) -> Self {
        Self::ALL
            .iter()
            .copied()
            .find(|k| format!("{:?}", k).eq_ignore_ascii_case(name))
            .unwrap_or(Self::Midnight)
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Midnight => "Midnight Black",
            Self::Lavender => "Lavender",
            Self::Ocean => "Ocean Blue",
            Self::Mint => "Sage Mint",
            Self::Coral => "Sunset Coral",
            Self::Charcoal => "Slate Charcoal",
            Self::Sakura => "Sakura Rose",
            Self::Cyberpunk => "Cyberpunk Neon",
            Self::Emerald => "Emerald Matrix",
            Self::Crimson => "Crimson Ruby",
            Self::Nord => "Nordic Frost",
            Self::TokyoNight => "Tokyo Night",
            Self::Dracula => "Dracula",
            Self::Gruvbox => "Gruvbox Retro",
            Self::Catppuccin => "Catppuccin Mocha",
            Self::Monokai => "Monokai Pro",
            Self::Vaporwave => "Vaporwave Neon",
            Self::Amber => "Amber Glow",
            Self::Teal => "Deep Teal",
            Self::RosePine => "Rosé Pine",
            Self::Solarized => "Solarized Dark",
            Self::Everforest => "Everforest",
            Self::Kanagawa => "Kanagawa Wave",
            Self::Ayu => "Ayu Mirage",
        }
    }

    pub fn swatch_color(&self) -> Color32 {
        match self {
            Self::Midnight => Color32::from_rgb(226, 226, 230),
            Self::Lavender => Color32::from_rgb(208, 188, 255),
            Self::Ocean => Color32::from_rgb(168, 199, 250),
            Self::Mint => Color32::from_rgb(128, 213, 180),
            Self::Coral => Color32::from_rgb(255, 183, 134),
            Self::Charcoal => Color32::from_rgb(198, 202, 212),
            Self::Sakura => Color32::from_rgb(255, 180, 207),
            Self::Cyberpunk => Color32::from_rgb(0, 240, 255),
            Self::Emerald => Color32::from_rgb(80, 230, 150),
            Self::Crimson => Color32::from_rgb(255, 100, 110),
            Self::Nord => Color32::from_rgb(136, 192, 208),
            Self::TokyoNight => Color32::from_rgb(122, 162, 247),
            Self::Dracula => Color32::from_rgb(255, 121, 198),
            Self::Gruvbox => Color32::from_rgb(250, 189, 47),
            Self::Catppuccin => Color32::from_rgb(203, 166, 247),
            Self::Monokai => Color32::from_rgb(255, 216, 102),
            Self::Vaporwave => Color32::from_rgb(255, 113, 206),
            Self::Amber => Color32::from_rgb(255, 191, 0),
            Self::Teal => Color32::from_rgb(78, 222, 212),
            Self::RosePine => Color32::from_rgb(235, 188, 186),
            Self::Solarized => Color32::from_rgb(38, 139, 210),
            Self::Everforest => Color32::from_rgb(167, 192, 128),
            Self::Kanagawa => Color32::from_rgb(126, 156, 216),
            Self::Ayu => Color32::from_rgb(255, 173, 102),
        }
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MonetTheme {
    pub kind: MonetPaletteKind,

    pub primary: Color32,
    pub on_primary: Color32,
    pub primary_container: Color32,
    pub on_primary_container: Color32,

    pub secondary: Color32,
    pub on_secondary: Color32,
    pub secondary_container: Color32,
    pub on_secondary_container: Color32,

    pub tertiary: Color32,
    pub on_tertiary: Color32,
    pub tertiary_container: Color32,
    pub on_tertiary_container: Color32,

    pub background: Color32,
    pub surface: Color32,
    pub surface_dim: Color32,
    pub surface_bright: Color32,
    pub surface_container_lowest: Color32,
    pub surface_container_low: Color32,
    pub surface_container: Color32,
    pub surface_container_high: Color32,
    pub surface_container_highest: Color32,

    pub on_surface: Color32,
    pub on_surface_variant: Color32,

    pub outline: Color32,
    pub outline_variant: Color32,

    pub error: Color32,
    pub error_container: Color32,
    pub on_error: Color32,
    pub on_error_container: Color32,

    pub success: Color32,
    pub success_container: Color32,
    pub on_success: Color32,
    pub on_success_container: Color32,

    pub warning: Color32,
    pub warning_container: Color32,
}

impl MonetTheme {
    pub fn from_kind(kind: MonetPaletteKind) -> Self {
        match kind {
            MonetPaletteKind::Midnight => Self {
                kind,
                primary: Color32::from_rgb(226, 226, 230),
                on_primary: Color32::from_rgb(24, 24, 26),
                primary_container: Color32::from_rgb(58, 58, 62),
                on_primary_container: Color32::from_rgb(238, 238, 242),

                secondary: Color32::from_rgb(196, 196, 202),
                on_secondary: Color32::from_rgb(28, 28, 30),
                secondary_container: Color32::from_rgb(66, 66, 70),
                on_secondary_container: Color32::from_rgb(226, 226, 230),

                tertiary: Color32::from_rgb(176, 176, 182),
                on_tertiary: Color32::from_rgb(24, 24, 26),
                tertiary_container: Color32::from_rgb(50, 50, 54),
                on_tertiary_container: Color32::from_rgb(218, 218, 222),

                background: Color32::from_rgb(14, 14, 15),
                surface: Color32::from_rgb(14, 14, 15),
                surface_dim: Color32::from_rgb(10, 10, 11),
                surface_bright: Color32::from_rgb(54, 54, 56),
                surface_container_lowest: Color32::from_rgb(10, 10, 10),
                surface_container_low: Color32::from_rgb(19, 19, 20),
                surface_container: Color32::from_rgb(28, 28, 30),
                surface_container_high: Color32::from_rgb(38, 38, 40),
                surface_container_highest: Color32::from_rgb(48, 48, 50),

                on_surface: Color32::from_rgb(242, 240, 247),
                on_surface_variant: Color32::from_rgb(190, 190, 196),

                outline: Color32::from_rgb(140, 140, 146),
                outline_variant: Color32::from_rgb(66, 66, 72),

                error: Color32::from_rgb(255, 180, 171),
                error_container: Color32::from_rgb(147, 0, 10),
                on_error: Color32::from_rgb(105, 0, 5),
                on_error_container: Color32::from_rgb(255, 218, 214),

                success: Color32::from_rgb(130, 217, 156),
                success_container: Color32::from_rgb(0, 83, 34),
                on_success: Color32::from_rgb(0, 57, 20),
                on_success_container: Color32::from_rgb(159, 246, 183),

                warning: Color32::from_rgb(255, 186, 40),
                warning_container: Color32::from_rgb(88, 63, 0),
            },

            MonetPaletteKind::Lavender => Self {
                kind,
                primary: Color32::from_rgb(208, 188, 255),
                on_primary: Color32::from_rgb(56, 30, 114),
                primary_container: Color32::from_rgb(79, 55, 139),
                on_primary_container: Color32::from_rgb(234, 221, 255),

                secondary: Color32::from_rgb(204, 194, 220),
                on_secondary: Color32::from_rgb(51, 45, 65),
                secondary_container: Color32::from_rgb(74, 68, 88),
                on_secondary_container: Color32::from_rgb(232, 222, 248),

                tertiary: Color32::from_rgb(239, 184, 200),
                on_tertiary: Color32::from_rgb(74, 37, 51),
                tertiary_container: Color32::from_rgb(99, 59, 72),
                on_tertiary_container: Color32::from_rgb(255, 216, 228),

                background: Color32::from_rgb(20, 18, 24),
                surface: Color32::from_rgb(20, 18, 24),
                surface_dim: Color32::from_rgb(16, 14, 20),
                surface_bright: Color32::from_rgb(59, 56, 62),
                surface_container_lowest: Color32::from_rgb(14, 12, 17),
                surface_container_low: Color32::from_rgb(29, 27, 32),
                surface_container: Color32::from_rgb(33, 31, 38),
                surface_container_high: Color32::from_rgb(43, 41, 48),
                surface_container_highest: Color32::from_rgb(54, 52, 59),

                on_surface: Color32::from_rgb(230, 224, 233),
                on_surface_variant: Color32::from_rgb(202, 196, 208),

                outline: Color32::from_rgb(147, 143, 153),
                outline_variant: Color32::from_rgb(73, 69, 79),

                error: Color32::from_rgb(255, 180, 171),
                error_container: Color32::from_rgb(147, 0, 10),
                on_error: Color32::from_rgb(105, 0, 5),
                on_error_container: Color32::from_rgb(255, 218, 214),

                success: Color32::from_rgb(130, 217, 156),
                success_container: Color32::from_rgb(0, 83, 34),
                on_success: Color32::from_rgb(0, 57, 20),
                on_success_container: Color32::from_rgb(159, 246, 183),

                warning: Color32::from_rgb(255, 186, 40),
                warning_container: Color32::from_rgb(88, 63, 0),
            },

            MonetPaletteKind::Ocean => Self {
                kind,
                primary: Color32::from_rgb(168, 199, 250),
                on_primary: Color32::from_rgb(4, 48, 104),
                primary_container: Color32::from_rgb(11, 87, 208),
                on_primary_container: Color32::from_rgb(211, 227, 253),

                secondary: Color32::from_rgb(189, 201, 225),
                on_secondary: Color32::from_rgb(37, 49, 70),
                secondary_container: Color32::from_rgb(60, 71, 94),
                on_secondary_container: Color32::from_rgb(218, 226, 249),

                tertiary: Color32::from_rgb(222, 188, 235),
                on_tertiary: Color32::from_rgb(63, 39, 76),
                tertiary_container: Color32::from_rgb(87, 62, 100),
                on_tertiary_container: Color32::from_rgb(244, 218, 255),

                background: Color32::from_rgb(17, 20, 24),
                surface: Color32::from_rgb(17, 20, 24),
                surface_dim: Color32::from_rgb(14, 16, 20),
                surface_bright: Color32::from_rgb(55, 58, 63),
                surface_container_lowest: Color32::from_rgb(12, 14, 17),
                surface_container_low: Color32::from_rgb(25, 28, 33),
                surface_container: Color32::from_rgb(30, 33, 37),
                surface_container_high: Color32::from_rgb(40, 43, 48),
                surface_container_highest: Color32::from_rgb(51, 54, 59),

                on_surface: Color32::from_rgb(225, 226, 232),
                on_surface_variant: Color32::from_rgb(194, 199, 207),

                outline: Color32::from_rgb(140, 145, 153),
                outline_variant: Color32::from_rgb(66, 71, 78),

                error: Color32::from_rgb(255, 180, 171),
                error_container: Color32::from_rgb(147, 0, 10),
                on_error: Color32::from_rgb(105, 0, 5),
                on_error_container: Color32::from_rgb(255, 218, 214),

                success: Color32::from_rgb(130, 217, 156),
                success_container: Color32::from_rgb(0, 83, 34),
                on_success: Color32::from_rgb(0, 57, 20),
                on_success_container: Color32::from_rgb(159, 246, 183),

                warning: Color32::from_rgb(255, 186, 40),
                warning_container: Color32::from_rgb(88, 63, 0),
            },

            MonetPaletteKind::Mint => Self {
                kind,
                primary: Color32::from_rgb(128, 213, 180),
                on_primary: Color32::from_rgb(0, 56, 41),
                primary_container: Color32::from_rgb(0, 82, 60),
                on_primary_container: Color32::from_rgb(168, 242, 210),

                secondary: Color32::from_rgb(180, 204, 192),
                on_secondary: Color32::from_rgb(32, 53, 44),
                secondary_container: Color32::from_rgb(54, 76, 66),
                on_secondary_container: Color32::from_rgb(208, 232, 220),

                tertiary: Color32::from_rgb(166, 206, 222),
                on_tertiary: Color32::from_rgb(16, 53, 66),
                tertiary_container: Color32::from_rgb(39, 76, 90),
                on_tertiary_container: Color32::from_rgb(194, 234, 250),

                background: Color32::from_rgb(16, 20, 18),
                surface: Color32::from_rgb(16, 20, 18),
                surface_dim: Color32::from_rgb(13, 17, 15),
                surface_bright: Color32::from_rgb(54, 59, 56),
                surface_container_lowest: Color32::from_rgb(11, 15, 13),
                surface_container_low: Color32::from_rgb(24, 29, 26),
                surface_container: Color32::from_rgb(28, 33, 30),
                surface_container_high: Color32::from_rgb(38, 44, 40),
                surface_container_highest: Color32::from_rgb(49, 55, 51),

                on_surface: Color32::from_rgb(224, 228, 224),
                on_surface_variant: Color32::from_rgb(193, 202, 196),

                outline: Color32::from_rgb(139, 149, 143),
                outline_variant: Color32::from_rgb(65, 75, 69),

                error: Color32::from_rgb(255, 180, 171),
                error_container: Color32::from_rgb(147, 0, 10),
                on_error: Color32::from_rgb(105, 0, 5),
                on_error_container: Color32::from_rgb(255, 218, 214),

                success: Color32::from_rgb(130, 217, 156),
                success_container: Color32::from_rgb(0, 83, 34),
                on_success: Color32::from_rgb(0, 57, 20),
                on_success_container: Color32::from_rgb(159, 246, 183),

                warning: Color32::from_rgb(255, 186, 40),
                warning_container: Color32::from_rgb(88, 63, 0),
            },

            MonetPaletteKind::Coral => Self {
                kind,
                primary: Color32::from_rgb(240, 170, 140),
                on_primary: Color32::from_rgb(58, 30, 20),
                primary_container: Color32::from_rgb(78, 48, 38),
                on_primary_container: Color32::from_rgb(255, 225, 215),

                secondary: Color32::from_rgb(229, 190, 177),
                on_secondary: Color32::from_rgb(67, 43, 34),
                secondary_container: Color32::from_rgb(93, 65, 55),
                on_secondary_container: Color32::from_rgb(255, 219, 208),

                tertiary: Color32::from_rgb(224, 197, 146),
                on_tertiary: Color32::from_rgb(63, 46, 7),
                tertiary_container: Color32::from_rgb(88, 69, 27),
                on_tertiary_container: Color32::from_rgb(253, 225, 172),

                background: Color32::from_rgb(22, 18, 16),
                surface: Color32::from_rgb(22, 18, 16),
                surface_dim: Color32::from_rgb(18, 15, 13),
                surface_bright: Color32::from_rgb(61, 56, 53),
                surface_container_lowest: Color32::from_rgb(15, 12, 11),
                surface_container_low: Color32::from_rgb(31, 26, 24),
                surface_container: Color32::from_rgb(36, 31, 28),
                surface_container_high: Color32::from_rgb(46, 40, 37),
                surface_container_highest: Color32::from_rgb(58, 51, 48),

                on_surface: Color32::from_rgb(232, 223, 220),
                on_surface_variant: Color32::from_rgb(212, 195, 190),

                outline: Color32::from_rgb(156, 140, 135),
                outline_variant: Color32::from_rgb(81, 68, 64),

                error: Color32::from_rgb(255, 180, 171),
                error_container: Color32::from_rgb(147, 0, 10),
                on_error: Color32::from_rgb(105, 0, 5),
                on_error_container: Color32::from_rgb(255, 218, 214),

                success: Color32::from_rgb(130, 217, 156),
                success_container: Color32::from_rgb(0, 83, 34),
                on_success: Color32::from_rgb(0, 57, 20),
                on_success_container: Color32::from_rgb(159, 246, 183),

                warning: Color32::from_rgb(255, 186, 40),
                warning_container: Color32::from_rgb(88, 63, 0),
            },

            MonetPaletteKind::Charcoal => Self {
                kind,
                primary: Color32::from_rgb(208, 212, 222),
                on_primary: Color32::from_rgb(42, 47, 56),
                primary_container: Color32::from_rgb(64, 70, 81),
                on_primary_container: Color32::from_rgb(230, 234, 244),

                secondary: Color32::from_rgb(194, 198, 208),
                on_secondary: Color32::from_rgb(44, 47, 54),
                secondary_container: Color32::from_rgb(66, 70, 78),
                on_secondary_container: Color32::from_rgb(222, 226, 236),

                tertiary: Color32::from_rgb(208, 194, 208),
                on_tertiary: Color32::from_rgb(54, 44, 54),
                tertiary_container: Color32::from_rgb(78, 66, 78),
                on_tertiary_container: Color32::from_rgb(236, 222, 236),

                background: Color32::from_rgb(18, 18, 20),
                surface: Color32::from_rgb(18, 18, 20),
                surface_dim: Color32::from_rgb(14, 14, 16),
                surface_bright: Color32::from_rgb(56, 56, 60),
                surface_container_lowest: Color32::from_rgb(12, 12, 14),
                surface_container_low: Color32::from_rgb(26, 26, 28),
                surface_container: Color32::from_rgb(30, 30, 33),
                surface_container_high: Color32::from_rgb(40, 40, 44),
                surface_container_highest: Color32::from_rgb(52, 52, 56),

                on_surface: Color32::from_rgb(226, 226, 230),
                on_surface_variant: Color32::from_rgb(196, 196, 202),

                outline: Color32::from_rgb(142, 142, 148),
                outline_variant: Color32::from_rgb(68, 68, 74),

                error: Color32::from_rgb(255, 180, 171),
                error_container: Color32::from_rgb(147, 0, 10),
                on_error: Color32::from_rgb(105, 0, 5),
                on_error_container: Color32::from_rgb(255, 218, 214),

                success: Color32::from_rgb(130, 217, 156),
                success_container: Color32::from_rgb(0, 83, 34),
                on_success: Color32::from_rgb(0, 57, 20),
                on_success_container: Color32::from_rgb(159, 246, 183),

                warning: Color32::from_rgb(255, 186, 40),
                warning_container: Color32::from_rgb(88, 63, 0),
            },

            MonetPaletteKind::Sakura => Self {
                kind,
                primary: Color32::from_rgb(255, 180, 207),
                on_primary: Color32::from_rgb(92, 17, 52),
                primary_container: Color32::from_rgb(123, 39, 76),
                on_primary_container: Color32::from_rgb(255, 216, 228),

                secondary: Color32::from_rgb(226, 189, 201),
                on_secondary: Color32::from_rgb(67, 41, 51),
                secondary_container: Color32::from_rgb(92, 63, 74),
                on_secondary_container: Color32::from_rgb(255, 217, 229),

                tertiary: Color32::from_rgb(240, 188, 166),
                on_tertiary: Color32::from_rgb(73, 39, 24),
                tertiary_container: Color32::from_rgb(99, 61, 44),
                on_tertiary_container: Color32::from_rgb(255, 219, 205),

                background: Color32::from_rgb(22, 17, 20),
                surface: Color32::from_rgb(22, 17, 20),
                surface_dim: Color32::from_rgb(17, 13, 16),
                surface_bright: Color32::from_rgb(62, 55, 59),
                surface_container_lowest: Color32::from_rgb(15, 11, 14),
                surface_container_low: Color32::from_rgb(30, 25, 28),
                surface_container: Color32::from_rgb(35, 30, 33),
                surface_container_high: Color32::from_rgb(46, 40, 44),
                surface_container_highest: Color32::from_rgb(58, 51, 55),

                on_surface: Color32::from_rgb(238, 222, 228),
                on_surface_variant: Color32::from_rgb(212, 194, 202),

                outline: Color32::from_rgb(158, 139, 147),
                outline_variant: Color32::from_rgb(82, 68, 75),

                error: Color32::from_rgb(255, 180, 171),
                error_container: Color32::from_rgb(147, 0, 10),
                on_error: Color32::from_rgb(105, 0, 5),
                on_error_container: Color32::from_rgb(255, 218, 214),

                success: Color32::from_rgb(130, 217, 156),
                success_container: Color32::from_rgb(0, 83, 34),
                on_success: Color32::from_rgb(0, 57, 20),
                on_success_container: Color32::from_rgb(159, 246, 183),

                warning: Color32::from_rgb(255, 186, 40),
                warning_container: Color32::from_rgb(88, 63, 0),
            },

            MonetPaletteKind::Cyberpunk => Self {
                kind,
                primary: Color32::from_rgb(0, 240, 255),
                on_primary: Color32::from_rgb(0, 54, 60),
                primary_container: Color32::from_rgb(0, 80, 88),
                on_primary_container: Color32::from_rgb(166, 248, 255),

                secondary: Color32::from_rgb(254, 222, 93),
                on_secondary: Color32::from_rgb(58, 48, 0),
                secondary_container: Color32::from_rgb(84, 71, 0),
                on_secondary_container: Color32::from_rgb(255, 238, 170),

                tertiary: Color32::from_rgb(255, 110, 210),
                on_tertiary: Color32::from_rgb(60, 0, 48),
                tertiary_container: Color32::from_rgb(88, 0, 70),
                on_tertiary_container: Color32::from_rgb(255, 215, 245),

                background: Color32::from_rgb(10, 14, 22),
                surface: Color32::from_rgb(10, 14, 22),
                surface_dim: Color32::from_rgb(8, 11, 18),
                surface_bright: Color32::from_rgb(45, 52, 66),
                surface_container_lowest: Color32::from_rgb(6, 9, 15),
                surface_container_low: Color32::from_rgb(18, 23, 34),
                surface_container: Color32::from_rgb(22, 28, 42),
                surface_container_high: Color32::from_rgb(32, 39, 56),
                surface_container_highest: Color32::from_rgb(44, 53, 72),

                on_surface: Color32::from_rgb(230, 240, 255),
                on_surface_variant: Color32::from_rgb(180, 200, 220),

                outline: Color32::from_rgb(120, 160, 180),
                outline_variant: Color32::from_rgb(50, 75, 95),

                error: Color32::from_rgb(255, 110, 110),
                error_container: Color32::from_rgb(147, 0, 10),
                on_error: Color32::from_rgb(105, 0, 5),
                on_error_container: Color32::from_rgb(255, 218, 214),

                success: Color32::from_rgb(80, 255, 170),
                success_container: Color32::from_rgb(0, 90, 45),
                on_success: Color32::from_rgb(0, 57, 20),
                on_success_container: Color32::from_rgb(160, 255, 200),

                warning: Color32::from_rgb(255, 220, 40),
                warning_container: Color32::from_rgb(90, 75, 0),
            },

            MonetPaletteKind::Emerald => Self {
                kind,
                primary: Color32::from_rgb(80, 230, 150),
                on_primary: Color32::from_rgb(0, 58, 30),
                primary_container: Color32::from_rgb(0, 85, 45),
                on_primary_container: Color32::from_rgb(150, 255, 190),

                secondary: Color32::from_rgb(175, 210, 190),
                on_secondary: Color32::from_rgb(28, 52, 40),
                secondary_container: Color32::from_rgb(50, 76, 62),
                on_secondary_container: Color32::from_rgb(205, 238, 218),

                tertiary: Color32::from_rgb(150, 220, 230),
                on_tertiary: Color32::from_rgb(10, 54, 62),
                tertiary_container: Color32::from_rgb(32, 78, 88),
                on_tertiary_container: Color32::from_rgb(180, 244, 254),

                background: Color32::from_rgb(12, 20, 16),
                surface: Color32::from_rgb(12, 20, 16),
                surface_dim: Color32::from_rgb(9, 16, 12),
                surface_bright: Color32::from_rgb(48, 60, 52),
                surface_container_lowest: Color32::from_rgb(7, 13, 10),
                surface_container_low: Color32::from_rgb(20, 30, 24),
                surface_container: Color32::from_rgb(24, 35, 28),
                surface_container_high: Color32::from_rgb(34, 47, 39),
                surface_container_highest: Color32::from_rgb(46, 60, 51),

                on_surface: Color32::from_rgb(220, 235, 225),
                on_surface_variant: Color32::from_rgb(185, 205, 195),

                outline: Color32::from_rgb(130, 155, 140),
                outline_variant: Color32::from_rgb(60, 80, 70),

                error: Color32::from_rgb(255, 180, 171),
                error_container: Color32::from_rgb(147, 0, 10),
                on_error: Color32::from_rgb(105, 0, 5),
                on_error_container: Color32::from_rgb(255, 218, 214),

                success: Color32::from_rgb(80, 230, 150),
                success_container: Color32::from_rgb(0, 85, 45),
                on_success: Color32::from_rgb(0, 58, 30),
                on_success_container: Color32::from_rgb(150, 255, 190),

                warning: Color32::from_rgb(255, 186, 40),
                warning_container: Color32::from_rgb(88, 63, 0),
            },

            MonetPaletteKind::Crimson => Self {
                kind,
                primary: Color32::from_rgb(255, 100, 110),
                on_primary: Color32::from_rgb(90, 0, 15),
                primary_container: Color32::from_rgb(135, 0, 25),
                on_primary_container: Color32::from_rgb(255, 210, 215),

                secondary: Color32::from_rgb(230, 180, 185),
                on_secondary: Color32::from_rgb(70, 35, 40),
                secondary_container: Color32::from_rgb(95, 55, 60),
                on_secondary_container: Color32::from_rgb(255, 215, 220),

                tertiary: Color32::from_rgb(245, 185, 150),
                on_tertiary: Color32::from_rgb(75, 40, 15),
                tertiary_container: Color32::from_rgb(105, 60, 30),
                on_tertiary_container: Color32::from_rgb(255, 220, 195),

                background: Color32::from_rgb(22, 14, 16),
                surface: Color32::from_rgb(22, 14, 16),
                surface_dim: Color32::from_rgb(18, 10, 12),
                surface_bright: Color32::from_rgb(64, 50, 53),
                surface_container_lowest: Color32::from_rgb(14, 8, 10),
                surface_container_low: Color32::from_rgb(30, 20, 23),
                surface_container: Color32::from_rgb(36, 25, 28),
                surface_container_high: Color32::from_rgb(48, 34, 38),
                surface_container_highest: Color32::from_rgb(60, 45, 50),

                on_surface: Color32::from_rgb(240, 220, 224),
                on_surface_variant: Color32::from_rgb(215, 190, 195),

                outline: Color32::from_rgb(160, 135, 140),
                outline_variant: Color32::from_rgb(85, 65, 70),

                error: Color32::from_rgb(255, 180, 171),
                error_container: Color32::from_rgb(147, 0, 10),
                on_error: Color32::from_rgb(105, 0, 5),
                on_error_container: Color32::from_rgb(255, 218, 214),

                success: Color32::from_rgb(130, 217, 156),
                success_container: Color32::from_rgb(0, 83, 34),
                on_success: Color32::from_rgb(0, 57, 20),
                on_success_container: Color32::from_rgb(159, 246, 183),

                warning: Color32::from_rgb(255, 186, 40),
                warning_container: Color32::from_rgb(88, 63, 0),
            },

            MonetPaletteKind::Nord => Self {
                kind,
                primary: Color32::from_rgb(136, 192, 208),
                on_primary: Color32::from_rgb(46, 52, 64),
                primary_container: Color32::from_rgb(76, 106, 145),
                on_primary_container: Color32::from_rgb(236, 239, 244),

                secondary: Color32::from_rgb(129, 161, 193),
                on_secondary: Color32::from_rgb(46, 52, 64),
                secondary_container: Color32::from_rgb(67, 76, 94),
                on_secondary_container: Color32::from_rgb(216, 222, 233),

                tertiary: Color32::from_rgb(180, 142, 173),
                on_tertiary: Color32::from_rgb(46, 52, 64),
                tertiary_container: Color32::from_rgb(76, 86, 106),
                on_tertiary_container: Color32::from_rgb(236, 239, 244),

                background: Color32::from_rgb(46, 52, 64),
                surface: Color32::from_rgb(46, 52, 64),
                surface_dim: Color32::from_rgb(40, 45, 56),
                surface_bright: Color32::from_rgb(76, 86, 106),
                surface_container_lowest: Color32::from_rgb(36, 41, 51),
                surface_container_low: Color32::from_rgb(52, 59, 73),
                surface_container: Color32::from_rgb(59, 66, 82),
                surface_container_high: Color32::from_rgb(67, 76, 94),
                surface_container_highest: Color32::from_rgb(76, 86, 106),

                on_surface: Color32::from_rgb(236, 239, 244),
                on_surface_variant: Color32::from_rgb(216, 222, 233),

                outline: Color32::from_rgb(143, 188, 187),
                outline_variant: Color32::from_rgb(76, 86, 106),

                error: Color32::from_rgb(191, 97, 106),
                error_container: Color32::from_rgb(147, 0, 10),
                on_error: Color32::from_rgb(46, 52, 64),
                on_error_container: Color32::from_rgb(236, 239, 244),

                success: Color32::from_rgb(163, 190, 140),
                success_container: Color32::from_rgb(0, 83, 34),
                on_success: Color32::from_rgb(46, 52, 64),
                on_success_container: Color32::from_rgb(236, 239, 244),

                warning: Color32::from_rgb(235, 203, 139),
                warning_container: Color32::from_rgb(88, 63, 0),
            },
            MonetPaletteKind::TokyoNight => Self {
                kind,
                primary: Color32::from_rgb(122, 162, 247),
                on_primary: Color32::from_rgb(26, 27, 38),
                primary_container: Color32::from_rgb(42, 60, 94),
                on_primary_container: Color32::from_rgb(192, 202, 245),

                secondary: Color32::from_rgb(187, 154, 247),
                on_secondary: Color32::from_rgb(26, 27, 38),
                secondary_container: Color32::from_rgb(61, 53, 86),
                on_secondary_container: Color32::from_rgb(224, 215, 255),

                tertiary: Color32::from_rgb(125, 207, 255),
                on_tertiary: Color32::from_rgb(26, 27, 38),
                tertiary_container: Color32::from_rgb(32, 58, 80),
                on_tertiary_container: Color32::from_rgb(212, 241, 255),

                background: Color32::from_rgb(26, 27, 38),
                surface: Color32::from_rgb(26, 27, 38),
                surface_dim: Color32::from_rgb(20, 20, 30),
                surface_bright: Color32::from_rgb(52, 59, 88),
                surface_container_lowest: Color32::from_rgb(18, 19, 27),
                surface_container_low: Color32::from_rgb(31, 35, 53),
                surface_container: Color32::from_rgb(36, 40, 59),
                surface_container_high: Color32::from_rgb(47, 52, 77),
                surface_container_highest: Color32::from_rgb(58, 64, 94),

                on_surface: Color32::from_rgb(192, 202, 245),
                on_surface_variant: Color32::from_rgb(169, 177, 214),

                outline: Color32::from_rgb(86, 95, 137),
                outline_variant: Color32::from_rgb(41, 46, 66),

                error: Color32::from_rgb(247, 118, 142),
                error_container: Color32::from_rgb(140, 28, 50),
                on_error: Color32::from_rgb(255, 255, 255),
                on_error_container: Color32::from_rgb(255, 218, 222),

                success: Color32::from_rgb(158, 206, 106),
                success_container: Color32::from_rgb(45, 80, 25),
                on_success: Color32::from_rgb(26, 27, 38),
                on_success_container: Color32::from_rgb(215, 245, 190),

                warning: Color32::from_rgb(224, 175, 104),
                warning_container: Color32::from_rgb(85, 60, 20),
            },

            MonetPaletteKind::Dracula => Self {
                kind,
                primary: Color32::from_rgb(255, 121, 198),
                on_primary: Color32::from_rgb(40, 42, 54),
                primary_container: Color32::from_rgb(100, 30, 75),
                on_primary_container: Color32::from_rgb(255, 215, 240),

                secondary: Color32::from_rgb(139, 233, 253),
                on_secondary: Color32::from_rgb(40, 42, 54),
                secondary_container: Color32::from_rgb(35, 75, 88),
                on_secondary_container: Color32::from_rgb(220, 250, 255),

                tertiary: Color32::from_rgb(189, 147, 249),
                on_tertiary: Color32::from_rgb(40, 42, 54),
                tertiary_container: Color32::from_rgb(70, 45, 105),
                on_tertiary_container: Color32::from_rgb(240, 225, 255),

                background: Color32::from_rgb(40, 42, 54),
                surface: Color32::from_rgb(40, 42, 54),
                surface_dim: Color32::from_rgb(33, 34, 44),
                surface_bright: Color32::from_rgb(68, 71, 90),
                surface_container_lowest: Color32::from_rgb(27, 28, 36),
                surface_container_low: Color32::from_rgb(45, 47, 61),
                surface_container: Color32::from_rgb(52, 55, 70),
                surface_container_high: Color32::from_rgb(62, 65, 83),
                surface_container_highest: Color32::from_rgb(68, 71, 90),

                on_surface: Color32::from_rgb(248, 248, 242),
                on_surface_variant: Color32::from_rgb(180, 182, 196),

                outline: Color32::from_rgb(98, 114, 164),
                outline_variant: Color32::from_rgb(58, 65, 92),

                error: Color32::from_rgb(255, 85, 85),
                error_container: Color32::from_rgb(120, 20, 20),
                on_error: Color32::from_rgb(255, 255, 255),
                on_error_container: Color32::from_rgb(255, 205, 205),

                success: Color32::from_rgb(80, 250, 123),
                success_container: Color32::from_rgb(20, 80, 35),
                on_success: Color32::from_rgb(40, 42, 54),
                on_success_container: Color32::from_rgb(205, 255, 220),

                warning: Color32::from_rgb(241, 250, 140),
                warning_container: Color32::from_rgb(80, 85, 25),
            },

            MonetPaletteKind::Gruvbox => Self {
                kind,
                primary: Color32::from_rgb(235, 185, 95),
                on_primary: Color32::from_rgb(40, 40, 40),
                primary_container: Color32::from_rgb(75, 58, 28),
                on_primary_container: Color32::from_rgb(245, 225, 185),

                secondary: Color32::from_rgb(215, 140, 85),
                on_secondary: Color32::from_rgb(40, 40, 40),
                secondary_container: Color32::from_rgb(70, 48, 32),
                on_secondary_container: Color32::from_rgb(255, 220, 190),

                tertiary: Color32::from_rgb(131, 165, 152),
                on_tertiary: Color32::from_rgb(40, 40, 40),
                tertiary_container: Color32::from_rgb(40, 60, 55),
                on_tertiary_container: Color32::from_rgb(215, 235, 230),

                background: Color32::from_rgb(40, 40, 40),
                surface: Color32::from_rgb(40, 40, 40),
                surface_dim: Color32::from_rgb(30, 30, 30),
                surface_bright: Color32::from_rgb(80, 73, 69),
                surface_container_lowest: Color32::from_rgb(25, 25, 25),
                surface_container_low: Color32::from_rgb(45, 44, 43),
                surface_container: Color32::from_rgb(50, 48, 47),
                surface_container_high: Color32::from_rgb(60, 56, 54),
                surface_container_highest: Color32::from_rgb(80, 73, 69),

                on_surface: Color32::from_rgb(235, 219, 178),
                on_surface_variant: Color32::from_rgb(189, 174, 147),

                outline: Color32::from_rgb(124, 111, 100),
                outline_variant: Color32::from_rgb(60, 56, 54),

                error: Color32::from_rgb(251, 73, 52),
                error_container: Color32::from_rgb(110, 20, 15),
                on_error: Color32::from_rgb(255, 255, 255),
                on_error_container: Color32::from_rgb(255, 205, 200),

                success: Color32::from_rgb(184, 187, 38),
                success_container: Color32::from_rgb(60, 65, 10),
                on_success: Color32::from_rgb(40, 40, 40),
                on_success_container: Color32::from_rgb(235, 240, 180),

                warning: Color32::from_rgb(250, 189, 47),
                warning_container: Color32::from_rgb(85, 60, 12),
            },

            MonetPaletteKind::Catppuccin => Self {
                kind,
                primary: Color32::from_rgb(203, 166, 247),
                on_primary: Color32::from_rgb(30, 30, 46),
                primary_container: Color32::from_rgb(70, 50, 95),
                on_primary_container: Color32::from_rgb(240, 225, 255),

                secondary: Color32::from_rgb(137, 180, 250),
                on_secondary: Color32::from_rgb(30, 30, 46),
                secondary_container: Color32::from_rgb(40, 60, 90),
                on_secondary_container: Color32::from_rgb(215, 235, 255),

                tertiary: Color32::from_rgb(245, 194, 231),
                on_tertiary: Color32::from_rgb(30, 30, 46),
                tertiary_container: Color32::from_rgb(85, 55, 75),
                on_tertiary_container: Color32::from_rgb(255, 230, 245),

                background: Color32::from_rgb(30, 30, 46),
                surface: Color32::from_rgb(30, 30, 46),
                surface_dim: Color32::from_rgb(24, 24, 37),
                surface_bright: Color32::from_rgb(69, 71, 90),
                surface_container_lowest: Color32::from_rgb(17, 17, 27),
                surface_container_low: Color32::from_rgb(35, 36, 54),
                surface_container: Color32::from_rgb(49, 50, 68),
                surface_container_high: Color32::from_rgb(69, 71, 90),
                surface_container_highest: Color32::from_rgb(88, 91, 112),

                on_surface: Color32::from_rgb(205, 214, 244),
                on_surface_variant: Color32::from_rgb(186, 194, 222),

                outline: Color32::from_rgb(108, 112, 134),
                outline_variant: Color32::from_rgb(69, 71, 90),

                error: Color32::from_rgb(243, 139, 168),
                error_container: Color32::from_rgb(120, 35, 55),
                on_error: Color32::from_rgb(30, 30, 46),
                on_error_container: Color32::from_rgb(255, 215, 225),

                success: Color32::from_rgb(166, 227, 161),
                success_container: Color32::from_rgb(40, 80, 40),
                on_success: Color32::from_rgb(30, 30, 46),
                on_success_container: Color32::from_rgb(220, 250, 215),

                warning: Color32::from_rgb(249, 226, 175),
                warning_container: Color32::from_rgb(85, 75, 35),
            },

            MonetPaletteKind::Monokai => Self {
                kind,
                primary: Color32::from_rgb(255, 216, 102),
                on_primary: Color32::from_rgb(39, 40, 34),
                primary_container: Color32::from_rgb(90, 75, 20),
                on_primary_container: Color32::from_rgb(255, 245, 190),

                secondary: Color32::from_rgb(255, 97, 136),
                on_secondary: Color32::from_rgb(39, 40, 34),
                secondary_container: Color32::from_rgb(100, 25, 45),
                on_secondary_container: Color32::from_rgb(255, 210, 220),

                tertiary: Color32::from_rgb(120, 220, 232),
                on_tertiary: Color32::from_rgb(39, 40, 34),
                tertiary_container: Color32::from_rgb(25, 75, 85),
                on_tertiary_container: Color32::from_rgb(210, 250, 255),

                background: Color32::from_rgb(39, 40, 34),
                surface: Color32::from_rgb(39, 40, 34),
                surface_dim: Color32::from_rgb(30, 31, 26),
                surface_bright: Color32::from_rgb(73, 72, 62),
                surface_container_lowest: Color32::from_rgb(24, 25, 21),
                surface_container_low: Color32::from_rgb(44, 45, 39),
                surface_container: Color32::from_rgb(52, 53, 46),
                surface_container_high: Color32::from_rgb(62, 63, 55),
                surface_container_highest: Color32::from_rgb(73, 72, 62),

                on_surface: Color32::from_rgb(248, 248, 242),
                on_surface_variant: Color32::from_rgb(170, 169, 160),

                outline: Color32::from_rgb(117, 113, 94),
                outline_variant: Color32::from_rgb(60, 59, 50),

                error: Color32::from_rgb(255, 97, 136),
                error_container: Color32::from_rgb(120, 20, 40),
                on_error: Color32::from_rgb(39, 40, 34),
                on_error_container: Color32::from_rgb(255, 210, 220),

                success: Color32::from_rgb(169, 220, 107),
                success_container: Color32::from_rgb(50, 75, 20),
                on_success: Color32::from_rgb(39, 40, 34),
                on_success_container: Color32::from_rgb(225, 250, 200),

                warning: Color32::from_rgb(252, 152, 103),
                warning_container: Color32::from_rgb(95, 45, 20),
            },

            MonetPaletteKind::Vaporwave => Self {
                kind,
                primary: Color32::from_rgb(255, 113, 206),
                on_primary: Color32::from_rgb(26, 9, 51),
                primary_container: Color32::from_rgb(105, 15, 80),
                on_primary_container: Color32::from_rgb(255, 215, 245),

                secondary: Color32::from_rgb(1, 205, 254),
                on_secondary: Color32::from_rgb(26, 9, 51),
                secondary_container: Color32::from_rgb(0, 70, 95),
                on_secondary_container: Color32::from_rgb(205, 248, 255),

                tertiary: Color32::from_rgb(185, 103, 255),
                on_tertiary: Color32::from_rgb(26, 9, 51),
                tertiary_container: Color32::from_rgb(70, 25, 110),
                on_tertiary_container: Color32::from_rgb(240, 220, 255),

                background: Color32::from_rgb(26, 9, 51),
                surface: Color32::from_rgb(26, 9, 51),
                surface_dim: Color32::from_rgb(19, 6, 38),
                surface_bright: Color32::from_rgb(60, 30, 105),
                surface_container_lowest: Color32::from_rgb(14, 4, 28),
                surface_container_low: Color32::from_rgb(35, 14, 66),
                surface_container: Color32::from_rgb(45, 18, 82),
                surface_container_high: Color32::from_rgb(58, 25, 102),
                surface_container_highest: Color32::from_rgb(72, 33, 122),

                on_surface: Color32::from_rgb(255, 240, 250),
                on_surface_variant: Color32::from_rgb(215, 195, 225),

                outline: Color32::from_rgb(140, 80, 180),
                outline_variant: Color32::from_rgb(70, 30, 95),

                error: Color32::from_rgb(255, 80, 120),
                error_container: Color32::from_rgb(130, 20, 50),
                on_error: Color32::from_rgb(255, 255, 255),
                on_error_container: Color32::from_rgb(255, 210, 225),

                success: Color32::from_rgb(5, 255, 161),
                success_container: Color32::from_rgb(0, 85, 50),
                on_success: Color32::from_rgb(26, 9, 51),
                on_success_container: Color32::from_rgb(200, 255, 235),

                warning: Color32::from_rgb(255, 251, 150),
                warning_container: Color32::from_rgb(95, 90, 20),
            },

            MonetPaletteKind::Amber => Self {
                kind,
                primary: Color32::from_rgb(255, 191, 0),
                on_primary: Color32::from_rgb(45, 27, 0),
                primary_container: Color32::from_rgb(75, 48, 0),
                on_primary_container: Color32::from_rgb(255, 223, 153),

                secondary: Color32::from_rgb(222, 196, 160),
                on_secondary: Color32::from_rgb(46, 36, 20),
                secondary_container: Color32::from_rgb(68, 54, 34),
                on_secondary_container: Color32::from_rgb(240, 222, 190),

                tertiary: Color32::from_rgb(242, 168, 118),
                on_tertiary: Color32::from_rgb(52, 26, 8),
                tertiary_container: Color32::from_rgb(78, 42, 20),
                on_tertiary_container: Color32::from_rgb(255, 219, 194),

                background: Color32::from_rgb(18, 17, 14),
                surface: Color32::from_rgb(18, 17, 14),
                surface_dim: Color32::from_rgb(14, 13, 11),
                surface_bright: Color32::from_rgb(56, 53, 46),
                surface_container_lowest: Color32::from_rgb(11, 10, 8),
                surface_container_low: Color32::from_rgb(21, 19, 16),
                surface_container: Color32::from_rgb(30, 28, 24),
                surface_container_high: Color32::from_rgb(40, 37, 32),
                surface_container_highest: Color32::from_rgb(50, 47, 41),

                on_surface: Color32::from_rgb(235, 228, 218),
                on_surface_variant: Color32::from_rgb(202, 194, 180),

                outline: Color32::from_rgb(146, 138, 124),
                outline_variant: Color32::from_rgb(72, 66, 56),

                error: Color32::from_rgb(255, 180, 171),
                error_container: Color32::from_rgb(147, 0, 10),
                on_error: Color32::from_rgb(105, 0, 5),
                on_error_container: Color32::from_rgb(255, 218, 214),

                success: Color32::from_rgb(130, 217, 156),
                success_container: Color32::from_rgb(0, 83, 34),
                on_success: Color32::from_rgb(0, 57, 20),
                on_success_container: Color32::from_rgb(159, 246, 183),

                warning: Color32::from_rgb(255, 186, 40),
                warning_container: Color32::from_rgb(88, 63, 0),
            },

            MonetPaletteKind::Teal => Self {
                kind,
                primary: Color32::from_rgb(78, 222, 212),
                on_primary: Color32::from_rgb(0, 55, 52),
                primary_container: Color32::from_rgb(0, 82, 78),
                on_primary_container: Color32::from_rgb(175, 245, 240),

                secondary: Color32::from_rgb(178, 212, 208),
                on_secondary: Color32::from_rgb(30, 52, 50),
                secondary_container: Color32::from_rgb(52, 75, 73),
                on_secondary_container: Color32::from_rgb(205, 235, 232),

                tertiary: Color32::from_rgb(165, 205, 230),
                on_tertiary: Color32::from_rgb(20, 50, 65),
                tertiary_container: Color32::from_rgb(45, 72, 90),
                on_tertiary_container: Color32::from_rgb(200, 235, 255),

                background: Color32::from_rgb(14, 20, 20),
                surface: Color32::from_rgb(14, 20, 20),
                surface_dim: Color32::from_rgb(11, 16, 16),
                surface_bright: Color32::from_rgb(50, 60, 60),
                surface_container_lowest: Color32::from_rgb(9, 14, 14),
                surface_container_low: Color32::from_rgb(19, 24, 24),
                surface_container: Color32::from_rgb(28, 34, 34),
                surface_container_high: Color32::from_rgb(38, 44, 44),
                surface_container_highest: Color32::from_rgb(48, 54, 54),

                on_surface: Color32::from_rgb(222, 232, 230),
                on_surface_variant: Color32::from_rgb(190, 204, 202),

                outline: Color32::from_rgb(135, 152, 150),
                outline_variant: Color32::from_rgb(62, 78, 76),

                error: Color32::from_rgb(255, 180, 171),
                error_container: Color32::from_rgb(147, 0, 10),
                on_error: Color32::from_rgb(105, 0, 5),
                on_error_container: Color32::from_rgb(255, 218, 214),

                success: Color32::from_rgb(130, 217, 156),
                success_container: Color32::from_rgb(0, 83, 34),
                on_success: Color32::from_rgb(0, 57, 20),
                on_success_container: Color32::from_rgb(159, 246, 183),

                warning: Color32::from_rgb(255, 186, 40),
                warning_container: Color32::from_rgb(88, 63, 0),
            },

            MonetPaletteKind::RosePine => Self {
                kind,
                primary: Color32::from_rgb(235, 188, 186),
                on_primary: Color32::from_rgb(45, 25, 30),
                primary_container: Color32::from_rgb(88, 48, 56),
                on_primary_container: Color32::from_rgb(255, 225, 225),

                secondary: Color32::from_rgb(156, 207, 216),
                on_secondary: Color32::from_rgb(25, 45, 50),
                secondary_container: Color32::from_rgb(49, 90, 105),
                on_secondary_container: Color32::from_rgb(215, 245, 250),

                tertiary: Color32::from_rgb(196, 167, 231),
                on_tertiary: Color32::from_rgb(40, 25, 60),
                tertiary_container: Color32::from_rgb(72, 50, 95),
                on_tertiary_container: Color32::from_rgb(240, 225, 255),

                background: Color32::from_rgb(25, 23, 36),
                surface: Color32::from_rgb(25, 23, 36),
                surface_dim: Color32::from_rgb(20, 18, 30),
                surface_bright: Color32::from_rgb(68, 64, 94),
                surface_container_lowest: Color32::from_rgb(17, 15, 25),
                surface_container_low: Color32::from_rgb(27, 25, 39),
                surface_container: Color32::from_rgb(36, 33, 52),
                surface_container_high: Color32::from_rgb(46, 42, 65),
                surface_container_highest: Color32::from_rgb(57, 53, 79),

                on_surface: Color32::from_rgb(224, 222, 244),
                on_surface_variant: Color32::from_rgb(160, 156, 184),

                outline: Color32::from_rgb(110, 106, 134),
                outline_variant: Color32::from_rgb(65, 60, 85),

                error: Color32::from_rgb(255, 180, 171),
                error_container: Color32::from_rgb(147, 0, 10),
                on_error: Color32::from_rgb(105, 0, 5),
                on_error_container: Color32::from_rgb(255, 218, 214),

                success: Color32::from_rgb(130, 217, 156),
                success_container: Color32::from_rgb(0, 83, 34),
                on_success: Color32::from_rgb(0, 57, 20),
                on_success_container: Color32::from_rgb(159, 246, 183),

                warning: Color32::from_rgb(255, 186, 40),
                warning_container: Color32::from_rgb(88, 63, 0),
            },

            MonetPaletteKind::Solarized => Self {
                kind,
                primary: Color32::from_rgb(38, 139, 210),
                on_primary: Color32::from_rgb(0, 20, 26),
                primary_container: Color32::from_rgb(15, 68, 98),
                on_primary_container: Color32::from_rgb(175, 225, 255),

                secondary: Color32::from_rgb(42, 161, 152),
                on_secondary: Color32::from_rgb(0, 43, 54),
                secondary_container: Color32::from_rgb(15, 80, 80),
                on_secondary_container: Color32::from_rgb(180, 240, 235),

                tertiary: Color32::from_rgb(181, 137, 0),
                on_tertiary: Color32::from_rgb(40, 30, 0),
                tertiary_container: Color32::from_rgb(85, 65, 0),
                on_tertiary_container: Color32::from_rgb(255, 235, 170),

                background: Color32::from_rgb(0, 34, 43),
                surface: Color32::from_rgb(0, 34, 43),
                surface_dim: Color32::from_rgb(0, 28, 36),
                surface_bright: Color32::from_rgb(30, 78, 92),
                surface_container_lowest: Color32::from_rgb(0, 24, 30),
                surface_container_low: Color32::from_rgb(0, 34, 43),
                surface_container: Color32::from_rgb(7, 44, 54),
                surface_container_high: Color32::from_rgb(14, 54, 66),
                surface_container_highest: Color32::from_rgb(22, 65, 78),

                on_surface: Color32::from_rgb(238, 232, 213),
                on_surface_variant: Color32::from_rgb(147, 161, 161),

                outline: Color32::from_rgb(101, 123, 131),
                outline_variant: Color32::from_rgb(40, 75, 85),

                error: Color32::from_rgb(255, 180, 171),
                error_container: Color32::from_rgb(147, 0, 10),
                on_error: Color32::from_rgb(105, 0, 5),
                on_error_container: Color32::from_rgb(255, 218, 214),

                success: Color32::from_rgb(130, 217, 156),
                success_container: Color32::from_rgb(0, 83, 34),
                on_success: Color32::from_rgb(0, 57, 20),
                on_success_container: Color32::from_rgb(159, 246, 183),

                warning: Color32::from_rgb(255, 186, 40),
                warning_container: Color32::from_rgb(88, 63, 0),
            },

            MonetPaletteKind::Everforest => Self {
                kind,
                primary: Color32::from_rgb(167, 192, 128),
                on_primary: Color32::from_rgb(35, 45, 30),
                primary_container: Color32::from_rgb(58, 75, 48),
                on_primary_container: Color32::from_rgb(220, 240, 195),

                secondary: Color32::from_rgb(131, 192, 146),
                on_secondary: Color32::from_rgb(25, 48, 35),
                secondary_container: Color32::from_rgb(45, 75, 55),
                on_secondary_container: Color32::from_rgb(205, 240, 215),

                tertiary: Color32::from_rgb(230, 152, 117),
                on_tertiary: Color32::from_rgb(50, 28, 15),
                tertiary_container: Color32::from_rgb(80, 48, 30),
                on_tertiary_container: Color32::from_rgb(255, 220, 200),

                background: Color32::from_rgb(35, 42, 46),
                surface: Color32::from_rgb(35, 42, 46),
                surface_dim: Color32::from_rgb(28, 34, 38),
                surface_bright: Color32::from_rgb(75, 86, 92),
                surface_container_lowest: Color32::from_rgb(24, 29, 32),
                surface_container_low: Color32::from_rgb(34, 41, 45),
                surface_container: Color32::from_rgb(44, 52, 57),
                surface_container_high: Color32::from_rgb(54, 63, 69),
                surface_container_highest: Color32::from_rgb(65, 75, 81),

                on_surface: Color32::from_rgb(211, 198, 170),
                on_surface_variant: Color32::from_rgb(167, 176, 164),

                outline: Color32::from_rgb(133, 146, 137),
                outline_variant: Color32::from_rgb(68, 78, 76),

                error: Color32::from_rgb(255, 180, 171),
                error_container: Color32::from_rgb(147, 0, 10),
                on_error: Color32::from_rgb(105, 0, 5),
                on_error_container: Color32::from_rgb(255, 218, 214),

                success: Color32::from_rgb(130, 217, 156),
                success_container: Color32::from_rgb(0, 83, 34),
                on_success: Color32::from_rgb(0, 57, 20),
                on_success_container: Color32::from_rgb(159, 246, 183),

                warning: Color32::from_rgb(255, 186, 40),
                warning_container: Color32::from_rgb(88, 63, 0),
            },

            MonetPaletteKind::Kanagawa => Self {
                kind,
                primary: Color32::from_rgb(126, 156, 216),
                on_primary: Color32::from_rgb(24, 32, 52),
                primary_container: Color32::from_rgb(44, 60, 95),
                on_primary_container: Color32::from_rgb(210, 225, 255),

                secondary: Color32::from_rgb(122, 168, 159),
                on_secondary: Color32::from_rgb(20, 42, 38),
                secondary_container: Color32::from_rgb(42, 75, 70),
                on_secondary_container: Color32::from_rgb(205, 235, 230),

                tertiary: Color32::from_rgb(230, 195, 132),
                on_tertiary: Color32::from_rgb(45, 36, 18),
                tertiary_container: Color32::from_rgb(78, 62, 34),
                on_tertiary_container: Color32::from_rgb(255, 235, 200),

                background: Color32::from_rgb(22, 22, 29),
                surface: Color32::from_rgb(22, 22, 29),
                surface_dim: Color32::from_rgb(17, 17, 23),
                surface_bright: Color32::from_rgb(62, 62, 80),
                surface_container_lowest: Color32::from_rgb(15, 15, 20),
                surface_container_low: Color32::from_rgb(25, 25, 33),
                surface_container: Color32::from_rgb(35, 35, 46),
                surface_container_high: Color32::from_rgb(46, 46, 59),
                surface_container_highest: Color32::from_rgb(58, 58, 74),

                on_surface: Color32::from_rgb(220, 215, 186),
                on_surface_variant: Color32::from_rgb(170, 166, 150),

                outline: Color32::from_rgb(114, 113, 130),
                outline_variant: Color32::from_rgb(65, 65, 80),

                error: Color32::from_rgb(255, 180, 171),
                error_container: Color32::from_rgb(147, 0, 10),
                on_error: Color32::from_rgb(105, 0, 5),
                on_error_container: Color32::from_rgb(255, 218, 214),

                success: Color32::from_rgb(130, 217, 156),
                success_container: Color32::from_rgb(0, 83, 34),
                on_success: Color32::from_rgb(0, 57, 20),
                on_success_container: Color32::from_rgb(159, 246, 183),

                warning: Color32::from_rgb(255, 186, 40),
                warning_container: Color32::from_rgb(88, 63, 0),
            },

            MonetPaletteKind::Ayu => Self {
                kind,
                primary: Color32::from_rgb(255, 173, 102),
                on_primary: Color32::from_rgb(48, 28, 12),
                primary_container: Color32::from_rgb(85, 48, 20),
                on_primary_container: Color32::from_rgb(255, 225, 200),

                secondary: Color32::from_rgb(115, 208, 255),
                on_secondary: Color32::from_rgb(15, 42, 58),
                secondary_container: Color32::from_rgb(30, 68, 92),
                on_secondary_container: Color32::from_rgb(205, 240, 255),

                tertiary: Color32::from_rgb(149, 230, 203),
                on_tertiary: Color32::from_rgb(18, 48, 40),
                tertiary_container: Color32::from_rgb(35, 75, 65),
                on_tertiary_container: Color32::from_rgb(205, 250, 240),

                background: Color32::from_rgb(25, 30, 40),
                surface: Color32::from_rgb(25, 30, 40),
                surface_dim: Color32::from_rgb(19, 23, 31),
                surface_bright: Color32::from_rgb(65, 74, 94),
                surface_container_lowest: Color32::from_rgb(17, 20, 27),
                surface_container_low: Color32::from_rgb(26, 31, 42),
                surface_container: Color32::from_rgb(36, 43, 57),
                surface_container_high: Color32::from_rgb(47, 55, 72),
                surface_container_highest: Color32::from_rgb(58, 67, 86),

                on_surface: Color32::from_rgb(203, 204, 198),
                on_surface_variant: Color32::from_rgb(160, 166, 178),

                outline: Color32::from_rgb(112, 120, 132),
                outline_variant: Color32::from_rgb(60, 68, 80),

                error: Color32::from_rgb(255, 180, 171),
                error_container: Color32::from_rgb(147, 0, 10),
                on_error: Color32::from_rgb(105, 0, 5),
                on_error_container: Color32::from_rgb(255, 218, 214),

                success: Color32::from_rgb(130, 217, 156),
                success_container: Color32::from_rgb(0, 83, 34),
                on_success: Color32::from_rgb(0, 57, 20),
                on_success_container: Color32::from_rgb(159, 246, 183),

                warning: Color32::from_rgb(255, 186, 40),
                warning_container: Color32::from_rgb(88, 63, 0),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG contrast ratio between two colors.
    fn contrast(a: Color32, b: Color32) -> f32 {
        let lum = |c: Color32| {
            let ch = |v: u8| {
                let v = v as f32 / 255.0;
                if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
            };
            0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
        };
        let (x, y) = (lum(a) + 0.05, lum(b) + 0.05);
        x.max(y) / x.min(y)
    }

    #[test]
    fn new_installs_start_on_midnight() {
        assert_eq!(MonetPaletteKind::ALL[0], MonetPaletteKind::Midnight);
        assert_eq!(MonetPaletteKind::from_config_name(&crate::config::Config::default().theme_palette), MonetPaletteKind::Midnight);
        assert_eq!(MonetPaletteKind::from_config_name("lavender"), MonetPaletteKind::Lavender);
        assert_eq!(MonetPaletteKind::from_config_name("gone"), MonetPaletteKind::Midnight);
    }

    /// The picker shows 1 to 4 columns; a multiple of 12 fills every row at each width.
    #[test]
    fn palettes_fill_the_picker_grid() {
        assert_eq!(MonetPaletteKind::ALL.len() % 12, 0);
        let names: std::collections::HashSet<_> = MonetPaletteKind::ALL.iter().map(|k| k.display_name()).collect();
        assert_eq!(names.len(), MonetPaletteKind::ALL.len());
    }

    #[test]
    fn every_palette_is_readable_and_layered() {
        for kind in MonetPaletteKind::ALL {
            let t = MonetTheme::from_kind(kind);
            assert_eq!(t.kind, kind);
            for (fg, bg, what) in [
                (t.on_surface, t.surface_container, "text"),
                (t.on_surface_variant, t.surface_container_high, "secondary text"),
                (t.on_primary, t.primary, "text on accent"),
                (t.on_primary_container, t.primary_container, "text on tonal"),
            ] {
                assert!(contrast(fg, bg) >= 4.5, "{:?}: {} contrast {:.1}", kind, what, contrast(fg, bg));
            }
            let layers = [
                t.surface_container_lowest,
                t.surface_container_low,
                t.surface_container,
                t.surface_container_high,
                t.surface_container_highest,
            ];
            for pair in layers.windows(2) {
                assert!(contrast(pair[0], t.on_surface) > contrast(pair[1], t.on_surface), "{:?}: surfaces don't step up", kind);
            }
        }
    }
}
