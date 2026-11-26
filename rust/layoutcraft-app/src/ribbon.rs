//! The ribbon as DATA (docs/34, step N3.1): the tabs, groups and commands of the web ribbon (`web/ribbon.js`, dumped to
//! `ribbon-web.json` by `visio-test-harness/dump-ribbon.mjs`), the icon files the web uses (`web/assets/icons`, embedded by
//! `ribbon_icons.rs`) and the map from a web command id to what the native app can do with it today.
use std::collections::BTreeMap;

use serde::Deserialize;

include!("ribbon_icons.rs");

const WEB_RIBBON: &str = include_str!("ribbon-web.json");

#[derive(Debug, Clone, Deserialize)]
pub struct Tab {
    pub name: String,
    #[serde(default)]
    pub note: Option<String>,
    pub groups: Vec<Group>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Group {
    pub name: String,
    pub items: Vec<Item>,
}

/// A command id, or a drop-down menu holding several ids (the web puts them behind one button; the native ribbon lists them in the group).
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Item {
    Command { id: String },
    Menu {
        #[allow(dead_code)] // the drop-down label of the web; the native ribbon lists the commands in the group
        menu: String,
        ids: Vec<String>,
    },
}

#[derive(Debug, Deserialize)]
struct Dump {
    tabs: Vec<Tab>,
    icons: BTreeMap<String, String>,
}

/// The ribbon: its tabs and the icon file name of each command id.
pub struct Ribbon {
    pub tabs: Vec<Tab>,
    pub icons: BTreeMap<String, String>,
}

impl Ribbon {
    pub fn load() -> Self {
        let dump: Dump = serde_json::from_str(WEB_RIBBON).expect("ribbon-web.json is valid");
        Self { tabs: dump.tabs, icons: dump.icons }
    }
}

/// The SVG text of an icon file (without the `.svg`), drawn in `color` instead of the files' black.
pub fn icon_svg(name: &str, color: &str) -> Option<String> {
    ICON_SVGS.iter().find(|(file, _)| *file == name).map(|(_, svg)| svg.replace("stroke=\"#000\"", &format!("stroke=\"{color}\"")))
}

// ---- the layout of a group, as the web ribbon does it (web/shell.html: `.rgitems{display:grid;grid-auto-flow:column;grid-template-rows:repeat(2,auto)}`) ----

/// Below this window width (logical px) the buttons that have an icon and a text show only the icon (the web's compact mode, `@media (max-width:1750px)`).
pub const COMPACT_BELOW: f32 = 1750.0;

pub fn is_compact(width: f32) -> bool {
    width < COMPACT_BELOW
}

/// The number of columns of a group of `items` cells laid out in two rows.
pub fn grid_columns(items: usize) -> usize {
    items.div_ceil(2)
}

/// The cell index of (column, row) in a group laid out column after column over two rows.
pub fn grid_index(column: usize, row: usize) -> usize {
    column * 2 + row
}

/// The (row, column) of item `index`: the first item top left, the second under it, the third top of the next column ...
pub fn grid_cell(index: usize) -> (usize, usize) {
    (index % 2, index / 2)
}

/// What the native app does for a web command id (the ids of `ribbon.js`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Native {
    Undo,
    Redo,
    SaveAs,
    Copy,
    Cut,
    Paste,
    Duplicate,
    Delete,
    Group,
    Ungroup,
    /// 2 to front, 3 to back, 4 forward, 5 backward
    Order(i32),
    RotateQuarter,
    RotateLeft,
    Flip(bool),
    /// (width, height): the other selected shapes take the first one's width and / or height
    SameSize(bool, bool),
    /// `Engine::group_op` 6..12: union, combine, fragment, intersect, subtract, join, trim
    ShapeOp(i32),
    /// 0 left, 1 centre, 2 right, 3 top, 4 middle, 5 bottom
    Align(u32),
    Distribute(bool),
    PageAdd,
    PageDuplicate,
    PageRename,
    PageDelete,
    PageMove(i32),
    Connector,
    Fit,
    /// The page as wide as the view.
    FitWidth,
    ActualSize,
    ZoomIn,
    ZoomOut,
    NewDrawing,
    OpenFile,
    DocProps,
    HelpShortcuts,
    HelpAbout,
    HelpMcp,
    TextBox,
    PageSetup,
    Comments,
    CheckDiagram,
    Compare,
    Reports,
    Borders,
    Export,
    ShapesPane,
    /// The Find window.
    Find,
    /// Show or hide the Layers window.
    Layers,
    /// Enter or leave the full-screen view.
    FullScreen,
    /// Show or hide the Shape Data window.
    ShapeData,
    /// The Hyperlink window of the selected shape.
    Hyperlink,
    /// Wait for a click in the Shapes panel and change the selected shapes into that one.
    ChangeShape,
    /// The Pan (hand) tool: a left-button drag moves the view.
    Hand,
    /// Snap dragged shapes to the grid / to other shapes / to the ruler guides, and the grid spacing (state of the app, like the web checkboxes).
    SnapGrid,
    SnapShapes,
    SnapGuides,
    GridStep,
    /// Show the grid lines on the page.
    ShowGrid,
    /// Show the rulers beside the canvas.
    ShowRulers,
}

impl Native {
    /// Commands that work without an open drawing.
    pub fn needs_document(self) -> bool {
        !matches!(self, Native::NewDrawing | Native::OpenFile | Native::HelpShortcuts | Native::HelpAbout | Native::HelpMcp)
    }
}

/// The native command behind a web command id, if the app has one.
pub fn native(id: &str) -> Option<Native> {
    Some(match id {
        "undo" => Native::Undo,
        "redo" => Native::Redo,
        "save" => Native::SaveAs,
        "copy" => Native::Copy,
        "cut" => Native::Cut,
        "paste" => Native::Paste,
        "dup" => Native::Duplicate,
        "del" => Native::Delete,
        "grp" => Native::Group,
        "ungrp" => Native::Ungroup,
        "zFront" => Native::Order(2),
        "zBack" => Native::Order(3),
        "zFwd" => Native::Order(4),
        "zBwd" => Native::Order(5),
        "rotR" => Native::RotateQuarter,
        "rotL" => Native::RotateLeft,
        "flipH" => Native::Flip(false),
        "flipV" => Native::Flip(true),
        "sameW" => Native::SameSize(true, false),
        "sameH" => Native::SameSize(false, true),
        "sameWH" => Native::SameSize(true, true),
        "opUnion" => Native::ShapeOp(6),
        "opCombine" => Native::ShapeOp(7),
        "opFragment" => Native::ShapeOp(8),
        "opIntersect" => Native::ShapeOp(9),
        "opSubtract" => Native::ShapeOp(10),
        "opJoin" => Native::ShapeOp(11),
        "opTrim" => Native::ShapeOp(12),
        "arrL" => Native::Align(0),
        "arrCx" => Native::Align(1),
        "arrRt" => Native::Align(2),
        "arrT" => Native::Align(3),
        "arrM" => Native::Align(4),
        "arrB" => Native::Align(5),
        "arrDistH" => Native::Distribute(false),
        "arrDistV" => Native::Distribute(true),
        "pageAdd" => Native::PageAdd,
        "pageDup" => Native::PageDuplicate,
        "pageRename" => Native::PageRename,
        "pageDel" => Native::PageDelete,
        "pageLeft" => Native::PageMove(-1),
        "pageRight" => Native::PageMove(1),
        "connectTool" => Native::Connector,
        "handTool" => Native::Hand,
        "snapGrid" => Native::SnapGrid,
        "snapShapes" => Native::SnapShapes,
        "snapGuides" => Native::SnapGuides,
        "showGrid" => Native::ShowGrid,
        "showRulers" => Native::ShowRulers,
        "gridStep" => Native::GridStep,
        "fit" => Native::Fit,
        "zoomWidth" => Native::FitWidth,
        "zoom100" => Native::ActualSize,
        "zin" => Native::ZoomIn,
        "zout" => Native::ZoomOut,
        "newBtn" => Native::NewDrawing,
        "file" => Native::OpenFile,
        "docPropsBtn" => Native::DocProps,
        "helpShortcuts" => Native::HelpShortcuts,
        "helpAbout" => Native::HelpAbout,
        "mcpInfo" => Native::HelpMcp,
        "insertTextBox" => Native::TextBox,
        "pageSetup" => Native::PageSetup,
        "checkDiagram" => Native::CheckDiagram,
        "compareBtn" => Native::Compare,
        "reportBtn" => Native::Reports,
        "exportBtn" => Native::Export,
        "paneShapes" => Native::ShapesPane,
        "findBtn" => Native::Find,
        "layersBtn" => Native::Layers,
        "fullScreen" => Native::FullScreen,
        "dataBtn" => Native::ShapeData,
        "insertHyperlink" => Native::Hyperlink,
        "changeShapeBtn" => Native::ChangeShape,
        "borderStyle" | "borderTitle" | "borderApply" | "borderRemove" => Native::Borders,
        "showComments" => Native::Comments,
        "newComment" => Native::Comments,
        _ => return None,
    })
}

/// A readable label for a web command id ("zFront" -> "Z front", "p:fontSize" -> "Font size").
pub fn label(id: &str) -> String {
    let id = id.strip_prefix("p:").unwrap_or(id);
    let known = match id {
        "newBtn" => "New",
        "file" => "Open",
        "docPropsBtn" => "Document Properties",
        "helpShortcuts" => "Keyboard shortcuts",
        "helpAbout" => "About",
        "mcpInfo" => "AI assistant (MCP)",
        "insertTextBox" => "Text Box",
        "pageSetup" => "Page Setup",
        "checkDiagram" => "Check Drawing",
        "compareBtn" => "Compare File",
        "reportBtn" => "Reports",
        "exportBtn" => "Export...",
        "paneShapes" => "Shapes",
        "borderStyle" => "Border Style",
        "borderTitle" => "Border Title",
        "borderApply" => "Add Border",
        "borderRemove" => "Remove Border",
        "showComments" => "Show Comments",
        "newComment" => "New Comment",
        "zin" => "Zoom In",
        "zout" => "Zoom Out",
        "paste" => "Paste",
        "cut" => "Cut",
        "copy" => "Copy",
        "painter" => "Copy Format",
        "connectTool" => "Connector",
        "connPointTool" => "Connection Point",
        "editPointsBtn" => "Edit Points",
        "fitCurveBtn" => "Fit Curve",
        "handTool" => "Pan",
        "grp" => "Group",
        "ungrp" => "Ungroup",
        "dup" => "Duplicate",
        "del" => "Delete",
        "stylesBtn" => "Styles",
        "changeShapeBtn" => "Change Shape",
        "findBtn" => "Find",
        "fullScreen" => "Full Screen",
        "dataBtn" => "Shape Fields",
        "layersBtn" => "Layers",
        "zFront" => "Bring to Front",
        "zFwd" => "Bring Forward",
        "zBwd" => "Send Backward",
        "zBack" => "Send to Back",
        "rotL" => "Rotate Left",
        "rotR" => "Rotate Right",
        "flipH" => "Flip Horizontal",
        "flipV" => "Flip Vertical",
        "sameW" => "Same Width",
        "sameH" => "Same Height",
        "sameWH" => "Same Size",
        "opUnion" => "Union",
        "opCombine" => "Combine",
        "opFragment" => "Fragment",
        "opIntersect" => "Intersect",
        "opSubtract" => "Subtract",
        "opJoin" => "Join",
        "opTrim" => "Trim",
        "arrL" => "Align Left",
        "arrCx" => "Align Center",
        "arrRt" => "Align Right",
        "arrT" => "Align Top",
        "arrM" => "Align Middle",
        "arrB" => "Align Bottom",
        "arrDistH" => "Distribute Horizontally",
        "arrDistV" => "Distribute Vertically",
        "pageAdd" => "Add Page",
        "pageDup" => "Duplicate Page",
        "pageRename" => "Rename Page",
        "pageDel" => "Delete Page",
        "pageLeft" => "Previous Page",
        "pageRight" => "Next Page",
        "undo" => "Undo",
        "redo" => "Redo",
        "save" => "Save",
        "fit" => "Fit to Window",
        "zoom100" => "100%",
        "zoomWidth" => "Page Width",
        _ => "",
    };
    if !known.is_empty() {
        return known.to_string();
    }
    // camelCase -> words
    let mut out = String::new();
    for (at, c) in id.chars().enumerate() {
        if c.is_uppercase() && at > 0 {
            out.push(' ');
        }
        out.push(if at == 0 { c.to_ascii_uppercase() } else { c });
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_group_fills_two_rows_column_after_column() {
        assert_eq!((0..5).map(grid_cell).collect::<Vec<_>>(), [(0, 0), (1, 0), (0, 1), (1, 1), (0, 2)]);
        assert_eq!([0, 1, 2, 3, 4, 7].map(grid_columns), [0, 1, 1, 2, 2, 4]);
        for index in 0..9 {
            let (row, column) = grid_cell(index);
            assert_eq!(grid_index(column, row), index);
        }
    }

    #[test]
    fn the_compact_mode_starts_below_1750_pixels() {
        assert!(is_compact(1296.0) && is_compact(1749.9));
        assert!(!is_compact(1750.0) && !is_compact(1920.0));
    }

    use super::*;

    #[test]
    fn the_ribbon_has_the_tabs_of_the_web_in_order() {
        let ribbon = Ribbon::load();
        let names: Vec<&str> = ribbon.tabs.iter().map(|tab| tab.name.as_str()).collect();
        assert_eq!(names, ["File", "Home", "Insert", "Draw", "Design", "Data", "Process", "Review", "View", "Help"]);
    }

    #[test]
    fn the_home_tab_has_the_groups_of_the_web() {
        let ribbon = Ribbon::load();
        let home = ribbon.tabs.iter().find(|tab| tab.name == "Home").unwrap();
        let groups: Vec<&str> = home.groups.iter().map(|group| group.name.as_str()).collect();
        assert_eq!(groups, ["Clipboard", "Font", "Paragraph", "Tools", "Shape Styles", "Arrange", "Editing"]);
    }

    /// The dump is in step with web/ribbon.js: every tab and group name of the dump is written in the web source (the exact check is
    /// `node visio-test-harness/dump-ribbon.mjs --check`, part of the harness).
    #[test]
    fn every_tab_and_group_name_is_written_in_the_web_source() {
        let source = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../web/ribbon.js")).expect("web/ribbon.js");
        for tab in Ribbon::load().tabs {
            assert!(source.contains(&format!("'{}'", tab.name)), "tab {}", tab.name);
            for group in tab.groups {
                assert!(source.contains(&format!("'{}'", group.name)), "group {} of {}", group.name, tab.name);
            }
        }
    }

    #[test]
    fn every_icon_of_the_dump_exists_and_is_drawn_in_the_requested_colour() {
        let ribbon = Ribbon::load();
        assert!(ribbon.icons.len() > 50);
        for (id, file) in &ribbon.icons {
            let svg = icon_svg(file, "#e6e6e6").unwrap_or_else(|| panic!("icon {file} of {id}"));
            assert!(svg.contains("stroke=\"#e6e6e6\"") && !svg.contains("stroke=\"#000\""), "{file}");
        }
    }

    #[test]
    fn the_native_commands_are_commands_of_the_web_ribbon() {
        let ribbon = Ribbon::load();
        let mut web_ids = std::collections::BTreeSet::new();
        for tab in &ribbon.tabs {
            for group in &tab.groups {
                for item in &group.items {
                    match item {
                        Item::Command { id } => {
                            web_ids.insert(id.clone());
                        }
                        Item::Menu { ids, .. } => web_ids.extend(ids.iter().cloned()),
                    }
                }
            }
        }
        // the quick access toolbar ids are not in TABS; the icon map names every command the web ribbon shows
        web_ids.extend(ribbon.icons.keys().cloned());
        web_ids.extend(["save", "undo", "redo"].map(String::from));
        let expected = [
            "undo", "redo", "save", "copy", "cut", "paste", "dup", "del", "grp", "ungrp", "zFront", "zBack", "zFwd", "zBwd", "rotR", "rotL", "flipH", "flipV", "sameW", "sameH", "sameWH", "opUnion", "opCombine", "opFragment", "opIntersect", "opSubtract", "opJoin", "opTrim", "arrL", "arrCx", "arrRt", "arrT",
            "arrM", "arrB", "arrDistH", "arrDistV", "pageAdd", "pageDup", "pageRename", "pageDel", "pageLeft", "pageRight", "connectTool", "handTool", "snapGrid", "snapShapes", "snapGuides", "showGrid", "showRulers", "gridStep", "fit", "zoomWidth", "zoom100", "zin", "zout", "newBtn", "file", "docPropsBtn", "helpShortcuts", "helpAbout", "mcpInfo", "insertTextBox", "pageSetup", "showComments", "newComment", "checkDiagram", "compareBtn", "reportBtn", "exportBtn", "paneShapes", "layersBtn", "fullScreen", "dataBtn", "findBtn", "insertHyperlink", "changeShapeBtn", "borderStyle", "borderTitle", "borderApply", "borderRemove",
        ];
        for id in expected {
            assert!(web_ids.contains(id), "{id} is a command id of the web ribbon");
            assert!(native(id).is_some(), "{id} is a native command");
        }
        // and nothing else is mapped: every id of the web that maps is in the expected list
        for id in &web_ids {
            if native(id).is_some() {
                assert!(expected.contains(&id.as_str()), "{id} maps to a native command but is not in the list this test knows");
            }
        }
        assert!(native("painter").is_none() && native("fitCurveBtn").is_none());
    }

    #[test]
    fn labels_are_readable() {
        assert_eq!(label("zFront"), "Bring to Front");
        assert_eq!(label("p:fontSize"), "Font Size");
        assert_eq!(label("showRulers"), "Show Rulers");
    }
}
