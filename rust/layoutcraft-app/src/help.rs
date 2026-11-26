//! Help: the product name, the About text and ONE table of the keyboard shortcuts of the native app (shown by Help > Keyboard shortcuts).
//! The wording follows the web shell's list (`web/backstage.js`, `SHORTCUTS`); the rows the web has and the native app does not have are not listed.

pub const PRODUCT: &str = "LayoutCraft";

pub fn about_text() -> String {
    format!(
        "A viewer and editor for drawings in the .vsdx format used by Visio. LayoutCraft is an independent project and is not affiliated with or endorsed by Microsoft. The drawing core is written in Rust; this is the native desktop application (version {}).\n\nFiles are opened and saved on your computer; the drawing is never uploaded.",
        env!("CARGO_PKG_VERSION")
    )
}

/// (group, rows of (keys, what it does)).
pub const SHORTCUTS: &[(&str, &[(&str, &str)])] = &[
    (
        "Editing",
        &[
            ("Ctrl+Z", "Undo"),
            ("Ctrl+Y or Ctrl+Shift+Z", "Redo"),
            ("Ctrl+X / Ctrl+C / Ctrl+V", "Cut / copy / paste"),
            ("Ctrl+D", "Duplicate"),
            ("Delete or Backspace", "Delete the selection"),
            ("F2 or Ctrl+E", "Edit the text of the selected shape"),
            ("Ctrl+S", "Save a copy"), ("Ctrl+B / Ctrl+I / Ctrl+U", "Bold / italic / underline"), ("Ctrl+Shift+L / C / R / J", "Align text left / centre / right / justify"),
        ],
    ),
    (
        "Shapes",
        &[
            ("Click, Shift+click", "Select, add to or remove from the selection"),
            ("Drag a shape", "Move it"),
            ("Drag a corner handle", "Resize it"),
            ("Ctrl+A", "Select all"), ("Tab / Shift+Tab", "Next / previous shape (a shape selected on the page)"),
            ("Arrow keys, with Shift", "Move the selection a small or a large step"),
        ],
    ),
    (
        "Arrange",
        &[
            ("Ctrl+G or Ctrl+Shift+G", "Group"),
            ("Ctrl+Shift+U", "Ungroup"),
            ("Ctrl+Shift+F / Ctrl+Shift+B", "Bring to front / send to back"),
            ("Ctrl+L / Ctrl+R, or [ / ]", "Turn left / right"),
            ("Ctrl+H / Ctrl+J, or H / V", "Flip horizontally / vertically"),
        ],
    ),
    (
        "Tools",
        &[
            ("Ctrl+1", "Pointer tool"),
            ("Ctrl+3", "Connector tool"),
            ("Ctrl+6 / Ctrl+8 / Ctrl+9", "Draw a line / rectangle / ellipse"),
        ],
    ),
    ("View", &[("Ctrl+F", "Find text on the page"), ("Ctrl+K", "Hyperlinks of the selected shape"), ("F5, Esc", "Full screen, and leave it"), ("Alt+F9", "Snap and Attach"), ("Ctrl+Shift+W", "Fit the page to the window"), ("Ctrl+Page Down / Ctrl+Page Up", "Next / previous page"), ("Ctrl+Plus / Ctrl+Minus", "Zoom in / out"), ("Mouse wheel, with Shift", "Scroll up and down, or sideways"), ("Ctrl+mouse wheel", "Zoom at the pointer"), ("Drag the empty page", "Pan the page")]),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_lists_every_shortcut_the_app_handles() {
        let keys: Vec<&str> = SHORTCUTS.iter().flat_map(|(_, rows)| rows.iter().map(|(keys, _)| *keys)).collect();
        for handled in ["Ctrl+Z", "Ctrl+Y or Ctrl+Shift+Z", "Ctrl+X / Ctrl+C / Ctrl+V", "Ctrl+D", "Delete or Backspace", "F2 or Ctrl+E", "Ctrl+S", "Ctrl+Shift+U", "Ctrl+L / Ctrl+R, or [ / ]", "Ctrl+1", "Ctrl+Shift+W"] {
            assert!(keys.contains(&handled), "{handled} is listed");
        }
    }

    /// The wording of the shared rows is the web's (`SHORTCUTS` in web/backstage.js).
    #[test]
    fn shared_rows_use_the_web_wording() {
        let web = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../web/backstage.js")).expect("web/backstage.js");
        for (keys, what) in [
            ("Ctrl+Z", "Undo"),
            ("Ctrl+Y or Ctrl+Shift+Z", "Redo"),
            ("Ctrl+X / Ctrl+C / Ctrl+V", "Cut / copy / paste"),
            ("Ctrl+D", "Duplicate"),
            ("Delete or Backspace", "Delete the selection"),
            ("Click, Shift+click", "Select, add to or remove from the selection"),
        ] {
            assert!(web.contains(&format!("['{keys}', '{what}']")), "the web has the row {keys} / {what}");
            assert!(SHORTCUTS.iter().any(|(_, rows)| rows.contains(&(keys, what))), "the native table has {keys} / {what}");
        }
    }

    #[test]
    fn about_names_the_product_and_the_privacy_line() {
        assert_eq!(PRODUCT, "LayoutCraft");
        assert!(about_text().contains("never uploaded") && about_text().contains(env!("CARGO_PKG_VERSION")));
    }
}
