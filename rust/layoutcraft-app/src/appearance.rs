//! The colours of the application window (View > Palette): dark, light or what the system asks for. The drawing keeps its own
//! colours; its themes are Design > Themes. The choice is remembered next to the list of opened stencils.
use super::LayoutCraftApp;
use eframe::egui::{self, Color32, ThemePreference};
use std::path::{Path, PathBuf};

/// The choices, as (stored name, shown name, preference).
const PALETTES: [(&str, &str, ThemePreference); 3] =
    [("system", "System", ThemePreference::System), ("light", "Light", ThemePreference::Light), ("dark", "Dark", ThemePreference::Dark)];

/// The colours the app paints itself, beside what the toolkit styles.
#[derive(Debug, Clone, Copy)]
pub(super) struct Chrome {
    /// The line colour of the ribbon icons, as an SVG colour.
    pub icon: &'static str,
    /// The area around the page, and the text on it.
    pub canvas: Color32,
    pub canvas_text: Color32,
    /// The ruler bands and their marks and numbers.
    pub ruler: Color32,
    pub ruler_ink: Color32,
    /// The name under a stencil shape.
    pub caption: Color32,
    /// Notices (unsaved changes, the picked shape) and problems (a missing font), as text on a panel.
    pub warning: Color32,
    pub error: Color32,
}

const DARK: Chrome = Chrome {
    icon: "#e6e6e6",
    canvas: Color32::from_rgb(28, 30, 34),
    canvas_text: Color32::from_gray(180),
    ruler: Color32::from_rgb(45, 48, 54),
    ruler_ink: Color32::from_gray(220),
    caption: Color32::from_gray(200),
    warning: Color32::from_rgb(255, 196, 0),
    error: Color32::from_rgb(255, 90, 90),
};

const LIGHT: Chrome = Chrome {
    icon: "#222222",
    canvas: Color32::from_rgb(205, 208, 213),
    canvas_text: Color32::from_gray(40),
    ruler: Color32::from_rgb(232, 234, 238),
    ruler_ink: Color32::from_gray(40),
    caption: Color32::from_gray(50),
    warning: Color32::from_rgb(125, 80, 0),
    error: Color32::from_rgb(165, 20, 20),
};

pub(super) fn chrome(dark: bool) -> Chrome {
    if dark {
        DARK
    } else {
        LIGHT
    }
}

fn shown_name(preference: ThemePreference) -> &'static str {
    PALETTES.iter().find(|(_, _, p)| *p == preference).map_or("System", |(_, name, _)| name)
}

/// The preference stored in `file`; the system's when there is none or it says something else.
fn read_preference(file: &Path) -> ThemePreference {
    let text = std::fs::read_to_string(file).unwrap_or_default();
    PALETTES.iter().find(|(stored, _, _)| *stored == text.trim()).map_or(ThemePreference::System, |(_, _, p)| *p)
}

fn write_preference(file: &Path, preference: ThemePreference) -> std::io::Result<()> {
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let stored = PALETTES.iter().find(|(_, _, p)| *p == preference).map_or("system", |(stored, _, _)| stored);
    std::fs::write(file, stored)
}

fn preference_file() -> Option<PathBuf> {
    super::stencil_memory_file().map(|file| file.with_file_name("appearance.txt"))
}

/// Use the palette chosen last time.
pub(super) fn restore(ctx: &egui::Context) {
    if let Some(file) = preference_file() {
        ctx.set_theme(read_preference(&file));
    }
}

impl LayoutCraftApp {
    /// The Palette choice in the View tab.
    pub(super) fn ribbon_palette(&mut self, ui: &mut egui::Ui) {
        let current = ui.ctx().options(|options| options.theme_preference);
        let mut chosen = current;
        let response = egui::ComboBox::from_id_salt("palette").selected_text(format!("Palette: {}", shown_name(current))).show_ui(ui, |ui| {
            for (_, name, preference) in PALETTES {
                ui.selectable_value(&mut chosen, preference, name);
            }
        });
        response.response.on_hover_text("Colours of the application (the drawing keeps its own); System follows the operating system, and is dark when it does not say");
        if chosen != current {
            ui.ctx().set_theme(chosen);
            self.status = match preference_file().map(|file| write_preference(&file, chosen)) {
                Some(Ok(())) => format!("Palette: {}", shown_name(chosen)),
                Some(Err(error)) => format!("Palette: {} (it could not be saved: {error})", shown_name(chosen)),
                None => format!("Palette: {} (it will not be remembered: no settings folder)", shown_name(chosen)),
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::Visuals;

    /// The contrast ratio of two opaque colours (WCAG relative luminance).
    fn contrast(a: Color32, b: Color32) -> f64 {
        let luminance = |c: Color32| {
            let channel = |v: u8| {
                let v = f64::from(v) / 255.0;
                if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
        };
        let (high, low) = (luminance(a).max(luminance(b)), luminance(a).min(luminance(b)));
        (high + 0.05) / (low + 0.05)
    }

    /// Both palettes: text at least 4.5:1 and icons at least 3:1 against what they lie on.
    #[test]
    fn text_and_icons_are_readable_in_both_palettes() {
        for (dark, visuals) in [(true, Visuals::dark()), (false, Visuals::light())] {
            let colours = chrome(dark);
            let panel = visuals.panel_fill;
            for text in [colours.caption, colours.warning, colours.error] {
                assert!(contrast(text, panel) >= 4.5, "dark {dark}: {text:?} on {panel:?}");
            }
            assert!(contrast(colours.canvas_text, colours.canvas) >= 4.5, "dark {dark}: canvas");
            assert!(contrast(colours.ruler_ink, colours.ruler) >= 4.5, "dark {dark}: ruler");
            let icon = Color32::from_hex(colours.icon).unwrap();
            for button in [panel, visuals.widgets.inactive.weak_bg_fill] {
                assert!(contrast(icon, button) >= 3.0, "dark {dark}: icon on {button:?}");
            }
        }
    }

    #[test]
    fn the_choice_is_remembered_and_anything_else_means_the_system() {
        let folder = std::env::temp_dir().join(format!("layoutcraft-palette-{}", std::process::id()));
        let file = folder.join("appearance.txt");
        assert_eq!(read_preference(&file), ThemePreference::System);
        for (_, _, preference) in PALETTES {
            write_preference(&file, preference).unwrap();
            assert_eq!(read_preference(&file), preference);
        }
        std::fs::write(&file, "ocean").unwrap();
        assert_eq!(read_preference(&file), ThemePreference::System);
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
