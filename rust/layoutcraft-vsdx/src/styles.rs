//! Document style sheets, theme palette and QuickStyle colour resolution.
//!
//! VSDX shape cells often carry only `Themed`/QuickStyle placeholders. The
//! concrete paint lives in `visio/document.xml` StyleSheets and the Office/Visio
//! theme part. This module mirrors the browser importer's inheritance order so
//! the editable model is not left with placeholder colours.
use roxmltree::Node;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

use crate::shapes::{shape_cells, shape_sections, Cell, Section};
use crate::{Package, Result};

pub(crate) const BUILTIN_COLORS: [&str; 25] = [
    "#000000", "#ffffff", "#ff0000", "#00ff00", "#0000ff", "#ffff00", "#ff00ff", "#00ffff",
    "#800000", "#008000", "#000080", "#808000", "#800080", "#008080", "#c0c0c0", "#e6e6e6",
    "#cdcdcd", "#b3b3b3", "#9a9a9a", "#808080", "#666666", "#4d4d4d", "#333333", "#1a1a1a",
    "#7f7f7f",
];

#[derive(Debug, Clone, Default)]
pub struct StyleSheet {
    pub id: String,
    pub name: String,
    pub fill_style: Option<String>,
    pub line_style: Option<String>,
    pub text_style: Option<String>,
    pub cells: BTreeMap<String, Cell>,
    pub sections: Vec<Section>,
}

#[derive(Debug, Clone, Default)]
pub struct DocumentStyles {
    pub sheets: BTreeMap<String, StyleSheet>,
    pub default_fill_style: Option<String>,
    pub default_line_style: Option<String>,
    pub default_text_style: Option<String>,
}

impl DocumentStyles {
    /// True when the shape's LineStyle is the document's Connector style (or
    /// inherits from it); those shapes resolve `THEMEVAL()` lines through the
    /// connector scheme rather than the general one.
    pub fn is_connector_style(&self, id: Option<&str>) -> bool {
        let mut current = id;
        let mut seen = BTreeSet::new();
        while let Some(current_id) = current {
            if current_id.is_empty() || !seen.insert(current_id.to_string()) {
                break;
            }
            let Some(sheet) = self.sheets.get(current_id) else {
                break;
            };
            if sheet.name.to_ascii_lowercase().contains("connector") {
                return true;
            }
            current = sheet.line_style.as_deref();
        }
        false
    }

    /// The font set by the document's root style ("No Style", id 0). A `Themed` font in a document
    /// without a theme part falls back to it (Visio draws the text in that face, not in a default).
    pub fn root_font(&self) -> Option<String> {
        let sheet = self.sheets.get("0")?;
        sheet
            .sections
            .iter()
            .filter(|section| section.name == "Character")
            .flat_map(|section| section.rows.iter())
            .find_map(|row| row.cells.get("Font"))
            .and_then(|cell| cell.value.clone())
            .filter(|value| !value.is_empty() && value != "Themed")
    }

    /// A numeric cell (text margins, for example) along a TextStyle chain, most specific first. A
    /// sheet that only inherits the cell (`Inh`) defers to its parent.
    pub fn text_style_number(&self, start: Option<&str>, name: &str) -> Option<f64> {
        let mut id = start.or(self.default_text_style.as_deref());
        let mut seen = BTreeSet::new();
        let mut inherited: Option<f64> = None;
        while let Some(current) = id {
            if current.is_empty() || !seen.insert(current.to_string()) {
                break;
            }
            let Some(sheet) = self.sheets.get(current) else {
                break;
            };
            if let Some(cell) = sheet.cells.get(name) {
                let value = cell.value.as_deref().and_then(|v| v.trim().parse::<f64>().ok());
                if cell.formula.as_deref() != Some("Inh") {
                    return value.or(inherited);
                }
                inherited = inherited.or(value);
            }
            id = sheet.text_style.as_deref();
        }
        inherited
    }

    pub fn load(package: &Package) -> Result<Self> {
        let doc = package.xml("visio/document.xml")?;
        let mut sheets = BTreeMap::new();
        for node in doc
            .descendants()
            .filter(|n| n.is_element() && n.tag_name().name() == "StyleSheet")
        {
            let Some(id) = node.attribute("ID") else {
                continue;
            };
            sheets.insert(
                id.to_string(),
                StyleSheet {
                    id: id.to_string(),
                    name: node
                        .attribute("NameU")
                        .or_else(|| node.attribute("Name"))
                        .unwrap_or("")
                        .to_string(),
                    fill_style: style_attr(node, "FillStyle"),
                    line_style: style_attr(node, "LineStyle"),
                    text_style: style_attr(node, "TextStyle"),
                    cells: shape_cells(node),
                    sections: shape_sections(node),
                },
            );
        }
        let settings = doc
            .descendants()
            .find(|n| n.is_element() && n.tag_name().name() == "DocumentSettings");
        Ok(Self {
            sheets,
            default_fill_style: settings.and_then(|s| style_attr(s, "DefaultFillStyle")),
            default_line_style: settings.and_then(|s| style_attr(s, "DefaultLineStyle")),
            default_text_style: settings.and_then(|s| style_attr(s, "DefaultTextStyle")),
        })
    }

    pub fn fill_rows(&self, id: Option<&str>) -> Vec<BTreeMap<String, Cell>> {
        self.style_rows(id, "FillStyle", "Fill", false)
    }

    pub fn line_rows(&self, id: Option<&str>) -> Vec<BTreeMap<String, Cell>> {
        self.style_rows(id, "LineStyle", "Line", false)
    }

    /// The Paragraph row of every style in a TextStyle chain, most specific first
    /// (alignment, indents and bullets a shape inherits from its text style).
    pub fn paragraph_rows(&self, id: Option<&str>) -> Vec<BTreeMap<String, Cell>> {
        self.style_rows(id, "TextStyle", "Paragraph", true)
    }

    /// The Character rows of a TextStyle chain, most specific first. Only the
    /// first row is used, matching a shape's own text look.
    pub fn text_rows(&self, id: Option<&str>) -> Vec<BTreeMap<String, Cell>> {
        self.style_rows(id, "TextStyle", "Character", true)
    }

    /// Gradient sections along a FillStyle chain, most specific first. Visio
    /// lets a shape's gradient rows omit `GradientStopColor`; each missing
    /// stop colour is inherited from the corresponding row of the style's
    /// gradient section.
    pub fn fill_gradient_sections(&self, id: Option<&str>) -> Vec<&[Section]> {
        self.gradient_chain(id, "FillStyle", "FillGradient")
    }

    /// Gradient sections along a LineStyle chain, most specific first.
    pub fn line_gradient_sections(&self, id: Option<&str>) -> Vec<&[Section]> {
        self.gradient_chain(id, "LineStyle", "LineGradient")
    }

    fn gradient_chain(
        &self,
        start: Option<&str>,
        prop: &str,
        section_name: &str,
    ) -> Vec<&[Section]> {
        let default = match prop {
            "FillStyle" => self.default_fill_style.as_deref(),
            _ => self.default_line_style.as_deref(),
        };
        let mut id = start.or(default);
        let mut seen = BTreeSet::new();
        let mut sections = Vec::new();
        while let Some(current) = id {
            if current.is_empty() || !seen.insert(current.to_string()) {
                break;
            }
            let Some(sheet) = self.sheets.get(current) else {
                break;
            };
            if let Some(section) = sheet
                .sections
                .iter()
                .find(|section| section.name == section_name)
            {
                sections.push(std::slice::from_ref(section));
            }
            id = match prop {
                "FillStyle" => sheet.fill_style.as_deref(),
                _ => sheet.line_style.as_deref(),
            };
        }
        sections
    }

    fn style_rows(
        &self,
        start: Option<&str>,
        prop: &str,
        section_name: &str,
        first_row_only: bool,
    ) -> Vec<BTreeMap<String, Cell>> {
        self.style_row_refs(start, prop, section_name, first_row_only)
            .into_iter()
            .cloned()
            .collect()
    }

    /// Borrowed fill rows of a FillStyle chain (no copies of the style sheets).
    pub fn fill_row_refs(&self, id: Option<&str>) -> Vec<&BTreeMap<String, Cell>> {
        self.style_row_refs(id, "FillStyle", "Fill", false)
    }

    /// Borrowed line rows of a LineStyle chain.
    pub fn line_row_refs(&self, id: Option<&str>) -> Vec<&BTreeMap<String, Cell>> {
        self.style_row_refs(id, "LineStyle", "Line", false)
    }

    fn style_row_refs(
        &self,
        start: Option<&str>,
        prop: &str,
        section_name: &str,
        first_row_only: bool,
    ) -> Vec<&BTreeMap<String, Cell>> {
        let default = match prop {
            "FillStyle" => self.default_fill_style.as_deref(),
            "LineStyle" => self.default_line_style.as_deref(),
            _ => self.default_text_style.as_deref(),
        };
        let mut id = start.or(default);
        let mut seen = BTreeSet::new();
        let mut rows = Vec::new();
        while let Some(current) = id {
            if current.is_empty() || !seen.insert(current.to_string()) {
                break;
            }
            let Some(sheet) = self.sheets.get(current) else {
                break;
            };
            if !first_row_only {
                rows.push(&sheet.cells);
            }
            if let Some(section) = sheet
                .sections
                .iter()
                .find(|section| section.name == section_name)
            {
                let take = if first_row_only {
                    1
                } else {
                    section.rows.len()
                };
                for row in section.rows.iter().take(take) {
                    rows.push(&row.cells);
                }
            }
            id = match prop {
                "FillStyle" => sheet.fill_style.as_deref(),
                "LineStyle" => sheet.line_style.as_deref(),
                _ => sheet.text_style.as_deref(),
            };
        }
        rows
    }
}

fn style_attr(node: Node<'_, '_>, name: &str) -> Option<String> {
    node.attribute(name)
        .map(str::to_string)
        .filter(|value| !value.is_empty())
}

#[derive(Debug, Clone, Default)]
pub struct ThemeStyle {
    /// `phClr`, a scheme slot name (`lt1`, `accent1`, ...), or `#rrggbb`.
    pub color: String,
    /// `<a:tint>` / `<a:shade>` modifiers in thousandths of a percent.
    pub modifiers: Vec<(String, f64)>,
    /// A gradient fill style, when the theme entry is a `gradFill`.
    pub gradient: Option<ThemeGradient>,
    /// Line width in points (`lnStyleLst` entries only).
    pub width_pt: Option<f64>,
    /// Dash pattern name from `prstDash` (`solid`, `dash`, ...).
    pub dash: Option<String>,
}

/// Resolved colour reference: colour token, tint/shade modifiers, opacity.
type ThemeColorRef = (String, Vec<(String, f64)>, f64);

#[derive(Debug, Clone, Default)]
pub struct ThemeGradient {
    pub stops: Vec<ThemeGradientStop>,
    /// DrawingML angle in radians.
    pub angle: f64,
}

#[derive(Debug, Clone, Default)]
pub struct ThemeGradientStop {
    pub color: String,
    pub modifiers: Vec<(String, f64)>,
    pub position: f64,
    pub opacity: f64,
}

#[derive(Debug, Clone, Default)]
pub struct ThemeEffectStyle {
    pub shadow: Option<ThemeShadowStyle>,
    pub glow: Option<ThemeGlowStyle>,
    pub soft_edge: Option<ThemeSoftEdgeStyle>,
    pub reflection: Option<ThemeReflectionStyle>,
}

#[derive(Debug, Clone, Default)]
pub struct ThemeShadowStyle {
    pub dist_emu: f64,
    pub dir: f64,
    pub blur_emu: f64,
    pub color: String,
    pub modifiers: Vec<(String, f64)>,
    pub alpha: f64,
}

#[derive(Debug, Clone, Default)]
pub struct ThemeGlowStyle {
    pub radius_emu: f64,
    pub color: String,
    pub modifiers: Vec<(String, f64)>,
    pub alpha: f64,
}

#[derive(Debug, Clone, Default)]
pub struct ThemeSoftEdgeStyle {
    pub radius_emu: f64,
}

#[derive(Debug, Clone, Default)]
pub struct ThemeReflectionStyle {
    pub blur_emu: f64,
    pub dist_emu: f64,
    pub alpha: f64,
}

#[derive(Debug, Clone, Default)]
pub struct VariationStyleScheme {
    pub variants: Vec<VariationStyle>,
    /// The variation's own `fillStyleLst`/`lnStyleLst`. Variant matrices
    /// (100-103) index these, not the global `fmtScheme` lists.
    pub fill_styles: Vec<ThemeStyle>,
    pub line_styles: Vec<ThemeStyle>,
}

#[derive(Debug, Clone, Default)]
pub struct VariationStyle {
    pub fill_idx: usize,
    pub line_idx: usize,
    pub effect_idx: usize,
}

#[derive(Debug, Clone, Default)]
pub struct QuickPaint {
    pub color: String,
    pub gradient: Option<Value>,
    pub opacity: f64,
}

#[derive(Debug, Clone, Default)]
pub struct QuickLine {
    pub color: String,
    pub width_pt: Option<f64>,
    pub dash: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct QuickShadow {
    /// Page-frame offset in inches (positive down/right).
    pub dx: f64,
    pub dy: f64,
    pub blur: f64,
    pub color: String,
    pub opacity: f64,
}

#[derive(Debug, Clone, Default)]
pub struct QuickGlow {
    pub radius: f64,
    pub color: String,
    pub opacity: f64,
}

#[derive(Debug, Clone, Default)]
pub struct QuickSoftEdge {
    pub radius: f64,
}

#[derive(Debug, Clone, Default)]
pub struct QuickReflection {
    pub blur: f64,
    pub distance: f64,
    pub opacity: f64,
}

#[derive(Debug, Clone, Default)]
pub struct QuickEffects {
    pub shadow: Option<QuickShadow>,
    pub glow: Option<QuickGlow>,
    pub soft_edge: Option<QuickSoftEdge>,
    pub reflection: Option<QuickReflection>,
}

#[derive(Debug, Clone)]
pub struct Theme {
    /// PageSheet `ShdwOffsetX/Y` (inches): where a legacy `ShdwPattern` shadow falls on this page.
    pub legacy_shadow_offset: Option<(f64, f64)>,
    pub colors: BTreeMap<String, String>,
    pub slots: BTreeMap<String, String>,
    pub variations: Vec<BTreeMap<String, String>>,
    pub active_variation: usize,
    pub name: String,
    pub scheme_enum: Option<String>,
    /// Line widths (points) from the theme's `fmtScheme/lnStyleLst`, in order.
    pub line_weights_pt: Vec<f64>,
    /// `fmtScheme/fillStyleLst` and `fmtScheme/lnStyleLst` colour entries.
    /// A bare `THEMEVAL()` in a fill/line cell resolves through these, which
    /// is why the same formula yields white in one theme (Sequence) and the
    /// accent colour in another (Linear).
    pub fill_styles: Vec<ThemeStyle>,
    pub line_styles: Vec<ThemeStyle>,
    /// Connector-scheme variants (`vt:fmtConnectorScheme`), used by shapes
    /// whose LineStyle resolves to the document's Connector style.
    pub connector_fill_styles: Vec<ThemeStyle>,
    pub connector_line_styles: Vec<ThemeStyle>,
    pub connector_line_weights_pt: Vec<f64>,
    /// `fmtScheme/effectStyleLst` entries (outer shadow, glow, soft edge,
    /// reflection) selected by `QuickStyleEffectsMatrix`.
    pub effect_styles: Vec<ThemeEffectStyle>,
    /// `variationStyleSchemeLst`; variant matrices 100-103 select a
    /// `fillIdx`/`lineIdx`/`effectIdx` from the active variation's entry.
    pub variation_style_schemes: Vec<VariationStyleScheme>,
    /// Active theme embellishment (2 for Sequence, 3 for Linear themes).
    pub embellishment: f64,
    /// `a:fontScheme` latin typefaces of the theme part: the body (minor) and heading (major) fonts
    /// a `Themed` Character font resolves to.
    pub minor_font: Option<String>,
    pub major_font: Option<String>,
    /// True when the package actually contains a theme part. Without one,
    /// Visio does not apply theme/quick-style colours.
    pub present: bool,
}

impl Default for Theme {
    fn default() -> Self {
        office_theme()
    }
}

impl Theme {
    /// The built-in Office theme (no theme part in the document).
    pub fn office() -> Theme {
        office_theme()
    }

    /// The theme's body font as a CSS font-family list (a `Themed` Character font), when the theme
    /// part names one.
    pub fn body_font_family(&self) -> Option<String> {
        if !self.present {
            return None;
        }
        self.minor_font
            .as_ref()
            .map(|face| format!("\"{face}\", Helvetica, Arial, sans-serif"))
    }

    pub fn load(package: &Package) -> Result<Self> {
        Self::load_for(package, None)
    }

    /// The theme of the theme part whose `schemeEnum` is `index` (a page's `ThemeIndex`); the same as `load` when `index` is `None` or no part carries that number.
    pub fn load_for(package: &Package, index: Option<u32>) -> Result<Self> {
        let doc = package.xml("visio/document.xml")?;
        let mut colors = BTreeMap::new();
        for entry in doc
            .descendants()
            .filter(|n| n.is_element() && n.tag_name().name() == "ColorEntry")
        {
            let Some(rgb) = entry.attribute("RGB") else {
                continue;
            };
            if let Some(hex) = normalize_hex(rgb) {
                colors.insert(entry.attribute("IX").unwrap_or("").to_string(), hex);
            }
        }

        let mut theme = Theme {
            colors,
            ..office_theme()
        };
        theme.slots.clear();
        theme.variations.clear();
        theme.active_variation = 0;
        theme.name.clear();
        theme.scheme_enum = None;

        let mut parts: Vec<String> = package
            .names()
            .filter(|name| {
                let lower = name.to_ascii_lowercase();
                lower.contains("theme/theme") && lower.ends_with(".xml")
            })
            .map(str::to_string)
            .collect();
        parts.sort();
        if index == Some(0) {
            // ThemeIndex 0: the page never got a theme, it looks as if the drawing had none (Visio, probe-page-themes-one.ps1)
            parts.clear();
        } else if let Some(index) = index {
            // the part whose COLOUR scheme carries this number (the first schemeEnum after `<a:clrScheme`; a part can name other numbers for its effect / font schemes)
            let colour_enum = |text: &str| -> Option<u32> {
                let at = text.find("<a:clrScheme")?;
                let e = text[at..].find("schemeEnum=")? + at + "schemeEnum=".len() + 1;
                text[e..].split(['"', '\'']).next()?.parse().ok()
            };
            let own: Vec<String> = parts.iter().filter(|part| package.text(part).map_or(false, |text| colour_enum(text) == Some(index))).cloned().collect();
            if !own.is_empty() {
                parts = own;
            }
        }

        for part in parts {
            let Ok(text) = package.text(&part) else {
                continue;
            };
            let is_visio = text.contains("variationClrSchemeLst") || text.contains("bkgnd");
            let Ok(theme_doc) = package.xml(&part) else {
                continue;
            };
            let slots = read_scheme(&theme_doc);
            if slots.is_empty() || (!is_visio && !theme.slots.is_empty()) {
                continue;
            }
            theme.slots = slots;
            theme.present = true;
            theme.name = theme_doc
                .descendants()
                .find(|n| n.is_element() && n.tag_name().name() == "theme")
                .and_then(|n| n.attribute("name"))
                .unwrap_or("")
                .to_string();
            theme.scheme_enum = theme_doc
                .descendants()
                .find(|n| n.is_element() && n.tag_name().name() == "schemeID")
                .and_then(|n| n.attribute("schemeEnum"))
                .map(str::to_string);
            // The style lists must come from the same part as the colour slots
            // (a package can carry several themes; the last Visio theme wins).
            let font_of = |tag: &str| {
                theme_doc
                    .descendants()
                    .find(|n| n.is_element() && n.tag_name().name() == tag)
                    .and_then(|font| {
                        font.children().find(|n| n.is_element() && n.tag_name().name() == "latin")
                    })
                    .and_then(|latin| latin.attribute("typeface"))
                    .filter(|face| !face.is_empty())
                    .map(str::to_string)
            };
            theme.minor_font = font_of("minorFont");
            theme.major_font = font_of("majorFont");
            theme.line_weights_pt.clear();
            theme.connector_line_styles.clear();
            theme.effect_styles.clear();
            theme.variation_style_schemes.clear();
            theme.read_extras(&theme_doc, package);
        }

        if theme.slots.is_empty() {
            return Ok(office_theme_with_colors(theme.colors));
        }
        if theme.variations.is_empty() {
            theme.variations = default_variations();
            theme.active_variation = 0;
        }
        if let Ok(document) = package.xml("visio/document.xml") {
            if let Some(index) = document
                .descendants()
                .filter(|node| node.is_element() && node.tag_name().name() == "Cell")
                .filter(|node| node.attribute("N") == Some("VariationColorIndex"))
                .filter_map(|node| node.attribute("V"))
                .filter_map(|value| value.parse::<usize>().ok())
                .find(|index| *index != 65534 && *index < theme.variations.len())
            {
                theme.active_variation = index;
            }
        }
        for (key, hex) in &theme.variations[theme.active_variation] {
            theme.slots.insert(key.clone(), hex.clone());
        }
        if theme.name.is_empty() {
            theme.name = "Office".to_string();
        }
        Ok(theme)
    }

    fn read_extras(&mut self, doc: &roxmltree::Document<'_>, package: &Package) {
        let embellishment = doc
            .descendants()
            .filter(|n| n.is_element() && n.tag_name().name() == "variationStyleScheme")
            .filter_map(|n| n.attribute("embellishment"))
            .filter_map(|value| value.parse::<f64>().ok())
            .fold(None, |best: Option<f64>, value| {
                Some(best.map_or(value, |best| best.max(value)))
            });
        if let Some(value) = embellishment {
            self.embellishment = value;
        }
        if self.line_weights_pt.is_empty() {
            if let Some(scheme) = doc
                .descendants()
                .find(|n| n.is_element() && n.tag_name().name() == "fmtScheme")
            {
                if let Some(list) = scheme
                    .children()
                    .find(|n| n.is_element() && n.tag_name().name() == "fillStyleLst")
                {
                    self.fill_styles = list
                        .children()
                        .filter(|n| n.is_element())
                        .filter_map(read_theme_style)
                        .collect();
                }
                if let Some(list) = scheme
                    .children()
                    .find(|n| n.is_element() && n.tag_name().name() == "lnStyleLst")
                {
                    for line in list
                        .children()
                        .filter(|n| n.is_element() && n.tag_name().name() == "ln")
                    {
                        if let Some(width) = line
                            .attribute("w")
                            .and_then(|width| width.parse::<f64>().ok())
                        {
                            // Theme line widths are EMU; 12700 EMU = 1 pt.
                            self.line_weights_pt.push(width / 12700.0);
                        }
                        if let Some(style) = read_theme_style(line) {
                            self.line_styles.push(style);
                        }
                    }
                }
            }
        }
        if self.connector_line_styles.is_empty() {
            if let Some(scheme) = doc
                .descendants()
                .find(|n| n.is_element() && n.tag_name().name() == "fmtConnectorScheme")
            {
                if let Some(list) = scheme
                    .children()
                    .find(|n| n.is_element() && n.tag_name().name() == "fillStyleLst")
                {
                    self.connector_fill_styles = list
                        .children()
                        .filter(|n| n.is_element())
                        .filter_map(read_theme_style)
                        .collect();
                }
                if let Some(list) = scheme
                    .children()
                    .find(|n| n.is_element() && n.tag_name().name() == "lnStyleLst")
                {
                    for line in list
                        .children()
                        .filter(|n| n.is_element() && n.tag_name().name() == "ln")
                    {
                        if let Some(width) = line
                            .attribute("w")
                            .and_then(|width| width.parse::<f64>().ok())
                        {
                            self.connector_line_weights_pt.push(width / 12700.0);
                        }
                        if let Some(style) = read_theme_style(line) {
                            self.connector_line_styles.push(style);
                        }
                    }
                }
            }
        }
        if self.effect_styles.is_empty() {
            if let Some(scheme) = doc
                .descendants()
                .find(|n| n.is_element() && n.tag_name().name() == "fmtScheme")
            {
                if let Some(list) = scheme
                    .children()
                    .find(|n| n.is_element() && n.tag_name().name() == "effectStyleLst")
                {
                    self.effect_styles = list
                        .children()
                        .filter(|n| n.is_element() && n.tag_name().name() == "effectStyle")
                        .filter_map(read_effect_style)
                        .collect();
                }
            }
        }
        if self.variation_style_schemes.is_empty() {
            self.variation_style_schemes = read_variation_style_schemes(doc);
        }
        if let Some(bg) = doc
            .descendants()
            .find(|n| n.is_element() && n.tag_name().name() == "bkgnd")
        {
            if let Some(hex) = first_theme_color(bg) {
                self.slots.insert("bkgnd".to_string(), hex);
            }
        }
        let Some(list) = doc
            .descendants()
            .find(|n| n.is_element() && n.tag_name().name() == "variationClrSchemeLst")
        else {
            return;
        };
        let mut variations = Vec::new();
        for scheme in list
            .children()
            .filter(|n| n.is_element() && n.tag_name().name() == "variationClrScheme")
        {
            let mut colors = BTreeMap::new();
            for child in scheme.children().filter(|n| n.is_element()) {
                let name = child.tag_name().name();
                let Some(index) = name.strip_prefix("varColor") else {
                    continue;
                };
                if let Some(hex) = first_theme_color(child) {
                    colors.insert(format!("variant{index}"), hex);
                }
            }
            variations.push(colors);
        }
        if variations.is_empty() {
            return;
        }
        self.variations = variations;

        let explicit = explicit_fill_scores(package);
        let mut chosen = 0;
        let mut best = 0;
        for (index, variation) in self.variations.iter().enumerate() {
            let score: usize = variation.values().filter_map(|hex| explicit.get(hex)).sum();
            if score > best {
                best = score;
                chosen = index;
            }
        }
        self.active_variation = chosen;
    }

    /// This theme with another colour variation active, for a page whose
    /// PageSheet sets its own `VariationColorIndex`. Out-of-range indices
    /// leave the theme unchanged.
    pub fn with_variation(&self, index: usize) -> Theme {
        let mut theme = self.clone();
        if index != theme.active_variation {
            if let Some(variation) = theme.variations.get(index).cloned() {
                theme.active_variation = index;
                for (key, hex) in variation {
                    theme.slots.insert(key, hex);
                }
            }
        }
        theme
    }

    pub fn to_json(&self) -> Value {
        json!({
            "name": if self.name.is_empty() { "Office" } else { &self.name },
            "schemeEnum": self.scheme_enum,
            "slots": &self.slots,
            "variations": &self.variations,
            "activeVariation": self.active_variation,
        })
    }

    pub fn base_color(&self) -> &str {
        self.slots
            .get("accent1")
            .or_else(|| self.slots.get("dk1"))
            .map(String::as_str)
            .unwrap_or("#3b6ea5")
    }

    /// Resolve a `QuickStyle*Color` cell. 0-8 index base theme colours;
    /// 100-106 index the active variation's seven colours.
    pub fn quick_color(&self, value: Option<f64>) -> Option<String> {
        if !self.present {
            return None;
        }
        let n = value?;
        if !n.is_finite() {
            return None;
        }
        if (100.0..=106.0).contains(&n) {
            return self
                .slots
                .get(&format!("variant{}", n as i64 - 99))
                .cloned();
        }
        let keys = [
            "dk1", "lt1", "accent1", "accent2", "accent3", "accent4", "accent5", "accent6", "bkgnd",
        ];
        keys.get(n as usize)
            .and_then(|key| self.slots.get(*key))
            .cloned()
    }

    /// A theme slot by the name used in Visio formulas. `fallback` is for
    /// `THEMEVAL`/`Theme()`; a bare cell reference must not invent a colour.
    pub fn slot(&self, name: &str, fallback: bool) -> Option<String> {
        let key = canon(name);
        let base = self.base_color().to_string();
        if !key.is_empty() {
            if let Some(direct) = self.slots.get(&key) {
                return Some(direct.clone());
            }
            if key == "accentcolor" {
                return Some(
                    self.slots
                        .get("accent1")
                        .cloned()
                        .unwrap_or_else(|| base.clone()),
                );
            }
            if let Some(index) = key.strip_prefix("accentcolor") {
                if matches!(index, "1" | "2" | "3" | "4" | "5" | "6") {
                    return Some(
                        self.slots
                            .get(&format!("accent{index}"))
                            .cloned()
                            .unwrap_or_else(|| base.clone()),
                    );
                }
            }
            if let Some(index) = key.strip_prefix("variantcolor") {
                return Some(
                    self.slots
                        .get(&format!("variant{index}"))
                        .cloned()
                        .or_else(|| {
                            index
                                .parse::<usize>()
                                .ok()
                                .map(|n| format!("accent{}", ((n + 5) % 6) + 1))
                                .and_then(|accent| self.slots.get(&accent).cloned())
                        })
                        .unwrap_or_else(|| base.clone()),
                );
            }
            match key.as_str() {
                "dark" => {
                    return Some(
                        self.slots
                            .get("dk1")
                            .or_else(|| self.slots.get("dk2"))
                            .cloned()
                            .unwrap_or_else(|| "#000000".to_string()),
                    );
                }
                "light" => {
                    return Some(
                        self.slots
                            .get("lt1")
                            .cloned()
                            .unwrap_or_else(|| "#ffffff".to_string()),
                    );
                }
                "backgroundcolor" => {
                    return Some(
                        self.slots
                            .get("bkgnd")
                            .or_else(|| self.slots.get("lt1"))
                            .cloned()
                            .unwrap_or_else(|| "#ffffff".to_string()),
                    );
                }
                "textcolor" => {
                    return Some(
                        self.slots
                            .get("dk2")
                            .cloned()
                            .unwrap_or_else(|| tint(&base, -50.0)),
                    );
                }
                "fillcolor" => {
                    return Some(
                        self.slots
                            .get("accent1")
                            .cloned()
                            .unwrap_or_else(|| base.clone()),
                    );
                }
                "fillcolor2" => {
                    return Some(
                        self.slots
                            .get("accent2")
                            .or_else(|| self.slots.get("accent1"))
                            .cloned()
                            .unwrap_or_else(|| base.clone()),
                    );
                }
                "connectorcolor" => {
                    return Some(tint(
                        self.slots
                            .get("accent1")
                            .map_or(base.as_str(), String::as_str),
                        -7.0,
                    ));
                }
                _ => {}
            }
        }
        fallback.then_some(base)
    }

    /// The width (points) a `THEMEVAL()` line-weight cell resolves to.
    /// `QuickStyleLineMatrix` values 1..N pick the matching theme line style;
    /// the unnumbered default (100) uses the theme's second style, which is
    /// what Visio applies to plain stencil bodies.
    pub fn themed_line_weight_pt(&self, matrix: Option<f64>) -> f64 {
        if self.line_weights_pt.is_empty() {
            return 1.0;
        }
        let index = self
            .matrix_style_index(matrix, "line", self.line_weights_pt.len())
            .unwrap_or_else(|| 1.min(self.line_weights_pt.len() - 1));
        self.line_weights_pt[index]
    }

    /// The colour a bare `THEMEVAL()` resolves to on a shape styled with the
    /// document's Connector style. Visio uses the connector scheme's second
    /// line entry with `phClr` bound to the theme's connector colour.
    pub fn themed_line_connector(&self) -> Option<String> {
        let style = self
            .connector_line_styles
            .get(1)
            .or_else(|| self.connector_line_styles.first())?;
        let ph = self
            .slots
            .get("variant1")
            .or_else(|| self.slots.get("accent1"))
            .cloned()
            .unwrap_or_else(|| "#a5a5a5".to_string());
        self.apply_theme_style(style, &ph)
    }

    /// Connector line colour with Visio's small shade applied for list/table
    /// separator lines (`#1998d6` rather than the raw variation colour).
    pub fn themed_line_connector_shaded(&self) -> Option<String> {
        self.themed_line_connector().map(|color| tint(&color, -7.0))
    }

    /// Theme property lookup used by ShapeSheet formulas such as
    /// `THEMEPROP("Embellishment")`.
    pub fn property(&self, name: &str) -> Option<f64> {
        match name.to_ascii_lowercase().as_str() {
            "embellishment" => Some(self.embellishment),
            "multiformat" => Some(0.0),
            _ => None,
        }
    }

    pub fn connector_line_weight_pt(&self) -> f64 {
        self.connector_line_weights_pt
            .get(1)
            .or_else(|| self.connector_line_weights_pt.first())
            .copied()
            .unwrap_or(1.0)
    }

    /// The colour a bare `THEMEVAL()` in `FillForegnd` resolves to: the
    /// theme's default fill style (`fillStyleLst`, third entry), with
    /// `phClr` bound to the accent colour.
    pub fn themed_fill(&self) -> Option<String> {
        if !self.present {
            return Some("#ffffff".to_string());
        }
        let style = self
            .fill_styles
            .get(2)
            .or_else(|| self.fill_styles.first())?;
        let ph = self
            .slots
            .get("accent1")
            .cloned()
            .unwrap_or_else(|| "#4472c4".to_string());
        self.apply_fill_style(style, &ph)
    }

    /// The colour a bare `THEMEVAL()` in `LineColor` resolves to: the theme's
    /// default line style (second `lnStyleLst` entry), with `phClr` bound to
    /// the active variation's first colour (Visio's default line colour).
    pub fn themed_line(&self) -> Option<String> {
        let style = self.line_styles.get(1).or_else(|| self.line_styles.first());
        match style {
            Some(style) => {
                let ph = self
                    .slots
                    .get("variant1")
                    .or_else(|| self.slots.get("dk1"))
                    .cloned()
                    .unwrap_or_else(|| "#a5a5a5".to_string());
                self.apply_theme_style(style, &ph)
            }
            None if self.present => Some("#a5a5a5".to_string()),
            None => Some("#a5a5a5".to_string()),
        }
    }

    /// A bare `THEMEVAL()` in a Character colour cell. Visio uses the theme's
    /// light foreground when the shape actually paints a fill, and the accent
    /// colour for unfilled labels (multiplicities, annotations). Without a
    /// theme part it uses plain black.
    pub fn themed_text(&self, has_fill: bool) -> Option<String> {
        if !self.present {
            return Some("#000000".to_string());
        }
        if has_fill {
            return Some(
                self.slots
                    .get("lt1")
                    .cloned()
                    .unwrap_or_else(|| "#ffffff".to_string()),
            );
        }
        Some(
            self.slots
                .get("accent1")
                .cloned()
                .unwrap_or_else(|| "#4472c4".to_string()),
        )
    }

    /// The colour of a LINE style (and of the connector styles): the older rule, kept because the golden models (Visio paint of the Jenkins-on-Azure lines, lumMod 75 percent)
    /// match it.
    fn apply_theme_style(&self, style: &ThemeStyle, ph: &str) -> Option<String> {
        self.resolve_color_ref_with(&style.color, &style.modifiers, ph, false)
    }

    /// The colour of a FILL style. Visio-measured (variants-compare.mjs, the page theme fixtures, the golden models): the fill colour of a SOLID theme style carries the
    /// DrawingML satMod / lumMod / lumOff in HSL (Retrospect: satMod 99 percent turns BD582C into RGB 188 88 45; Integral and Organic variants within 1), the fill colour of a
    /// GRADIENT style is the base colour (Slice, Ion: lumMod 155 / 147 percent on the first stop do not show in FillForegnd), so a gradient style keeps the older rule.
    fn apply_fill_style(&self, style: &ThemeStyle, ph: &str) -> Option<String> {
        self.resolve_color_ref_with(&style.color, &style.modifiers, ph, style.gradient.is_none())
    }

    fn resolve_color_ref(
        &self,
        color: &str,
        modifiers: &[(String, f64)],
        ph: &str,
    ) -> Option<String> {
        self.resolve_color_ref_with(color, modifiers, ph, false)
    }

    /// `hsl`: lumMod / lumOff / satMod / satOff in HSL (DrawingML); otherwise the older rule (lumMod as a linear-light scale limited to 100 percent, lumOff linear, no satMod).
    fn resolve_color_ref_with(
        &self,
        color: &str,
        modifiers: &[(String, f64)],
        ph: &str,
        hsl: bool,
    ) -> Option<String> {
        let mut color = if color.eq_ignore_ascii_case("phClr") {
            ph.to_string()
        } else if normalize_hex(color).is_some() {
            normalize_hex(color)?
        } else {
            self.slots.get(&canon(color))?.clone()
        };
        for (name, value) in modifiers {
            match name.as_str() {
                // DrawingML tint/shade are linear-light blends: tint keeps
                // `p` of the colour and mixes the rest with white; shade and
                // lumMod scale the linear channels by `p`.
                "tint" => color = theme_tint(&color, value / 100000.0),
                "shade" => color = theme_shade(&color, value / 100000.0),
                // DrawingML luminance / saturation modifiers work on the HSL values of the colour
                "lumMod" if hsl => color = hsl_modify(&color, 1.0, value / 100000.0, 0.0, 0.0),
                "lumOff" if hsl => color = hsl_modify(&color, 1.0, 1.0, value / 100000.0, 0.0),
                "satMod" if hsl => color = hsl_modify(&color, value / 100000.0, 1.0, 0.0, 0.0),
                "satOff" if hsl => color = hsl_modify(&color, 1.0, 1.0, 0.0, value / 100000.0),
                "lumMod" => color = theme_shade(&color, value / 100000.0),
                "lumOff" => color = theme_lum_off(&color, value / 100000.0),
                _ => {}
            }
        }
        Some(color)
    }

    /// Resolve a QuickStyle matrix cell to an index in the matching theme
    /// list. Values 1-6 index `fillStyleLst`/`lnStyleLst`/`effectStyleLst`
    /// directly; 100-103 select a variant entry from the active variation's
    /// `variationStyleScheme`.
    fn matrix_style_index(&self, matrix: Option<f64>, kind: &str, len: usize) -> Option<usize> {
        let value = matrix?.round();
        let index = if (1.0..=6.0).contains(&value) {
            value as usize - 1
        } else if (100.0..=103.0).contains(&value) {
            let scheme_index = self
                .active_variation
                .min(self.variation_style_schemes.len().saturating_sub(1));
            let scheme = self.variation_style_schemes.get(scheme_index)?;
            let variant = scheme.variants.get(value as usize - 100)?;
            match kind {
                "fill" => variant.fill_idx,
                "line" => variant.line_idx,
                "effect" => variant.effect_idx,
                _ => return None,
            }
            .saturating_sub(1)
        } else {
            return None;
        };
        (index < len).then_some(index)
    }

    fn gradient_json(&self, gradient: &ThemeGradient, ph: &str) -> Value {
        let stops: Vec<Value> = gradient
            .stops
            .iter()
            .filter_map(|stop| {
                let color = self.resolve_color_ref(&stop.color, &stop.modifiers, ph)?;
                Some(json!({
                    "color": color,
                    "opacity": stop.opacity.clamp(0.0, 1.0),
                    "position": stop.position.clamp(0.0, 1.0),
                }))
            })
            .collect();
        json!({ "angle": gradient.angle, "stops": stops })
    }

    /// Resolve a QuickStyle matrix to the theme style entry it selects.
    /// Variant matrices (100-103) index the active variation's own
    /// `fillStyleLst`/`lnStyleLst`; the global lists are only the fallback.
    fn quick_style(&self, matrix: Option<f64>, kind: &str) -> Option<&ThemeStyle> {
        let value = matrix?.round();
        if (1.0..=6.0).contains(&value) {
            let list = match kind {
                "line" => &self.line_styles,
                _ => &self.fill_styles,
            };
            return list.get(value as usize - 1);
        }
        if (100.0..=103.0).contains(&value) {
            let scheme_index = self
                .active_variation
                .min(self.variation_style_schemes.len().saturating_sub(1));
            let scheme = self.variation_style_schemes.get(scheme_index)?;
            let variant = scheme.variants.get(value as usize - 100)?;
            let (index, list) = match kind {
                "line" => (variant.line_idx, &scheme.line_styles),
                _ => (variant.fill_idx, &scheme.fill_styles),
            };
            if let Some(style) = list.get(index.saturating_sub(1)) {
                return Some(style);
            }
            let list = match kind {
                "line" => &self.line_styles,
                _ => &self.fill_styles,
            };
            return list.get(index.saturating_sub(1));
        }
        None
    }

    pub fn quick_fill_paint(&self, matrix: Option<f64>, color: Option<f64>) -> Option<QuickPaint> {
        let style = self.quick_style(matrix, "fill")?;
        let ph = color
            .and_then(|value| self.quick_color(Some(value)))
            .or_else(|| self.slots.get("variant1").cloned())
            .or_else(|| self.slots.get("accent1").cloned())?;
        let color = self.apply_fill_style(style, &ph)?;
        let gradient = style
            .gradient
            .as_ref()
            .map(|gradient| self.gradient_json(gradient, &ph));
        Some(QuickPaint {
            color,
            gradient,
            opacity: 1.0,
        })
    }

    /// `variant_line` is the shape's QuickStyleVariation bit 4: the line follows
    /// the theme variant (Visio-verified on the Bank Account leader lines, whose
    /// sibling shapes without the bit keep the theme's literal white line).
    pub fn quick_line_paint(
        &self,
        matrix: Option<f64>,
        color: Option<f64>,
        variant_line: bool,
    ) -> Option<QuickLine> {
        let style = self.quick_style(matrix, "line")?;
        let ph = color
            .and_then(|value| self.quick_color(Some(value)))
            .or_else(|| self.slots.get("variant1").cloned())
            .or_else(|| self.slots.get("dk1").cloned())?;
        // Visio's QuickStyleLineColor cell is authoritative for the colour;
        // the line-style list supplies width/dash. Some Visio themes store
        // literal white in lnStyleLst entries 2-6, but Visio still paints the
        // variant colour (UML state borders are green, not white).
        // Explicit matrices 1-6 (verified against Visio) take the style's own
        // colour even when it is a literal white; the variation matrices
        // 100-103 paint the variant colour instead.
        let variation_matrix = variant_line
            && matrix.is_some_and(|value| (100.0..=103.0).contains(&value.round()));
        let color = if style.color.eq_ignore_ascii_case("phClr")
            || (color.is_none() && !variation_matrix)
        {
            self.apply_theme_style(style, &ph)?
        } else {
            ph
        };
        let dash = style.dash.as_deref().and_then(|value| match value {
            "dash" | "sysDash" | "lgDash" | "dashDot" | "lgDashDot" => Some("dashed"),
            "dot" | "sysDot" => Some("dotted"),
            _ => None,
        });
        Some(QuickLine {
            color,
            width_pt: style.width_pt,
            dash: dash.map(str::to_string),
        })
    }

    pub fn quick_effect_paint(
        &self,
        matrix: Option<f64>,
        shadow_color: Option<f64>,
    ) -> Option<QuickEffects> {
        let index = self.matrix_style_index(matrix, "effect", self.effect_styles.len())?;
        let style = &self.effect_styles[index];
        let ph = shadow_color
            .and_then(|value| self.quick_color(Some(value)))
            .or_else(|| self.slots.get("dk1").cloned())
            .unwrap_or_else(|| "#000000".to_string());
        let mut effects = QuickEffects::default();
        if let Some(shadow) = &style.shadow {
            if let Some(color) = self.resolve_color_ref(&shadow.color, &shadow.modifiers, &ph) {
                let distance = shadow.dist_emu / 914400.0;
                let direction = shadow.dir / 60000.0 * std::f64::consts::PI / 180.0;
                effects.shadow = Some(QuickShadow {
                    dx: distance * direction.cos(),
                    dy: distance * direction.sin(),
                    blur: shadow.blur_emu / 914400.0,
                    color,
                    opacity: shadow.alpha.clamp(0.0, 1.0),
                });
            }
        }
        if let Some(glow) = &style.glow {
            if let Some(color) = self.resolve_color_ref(&glow.color, &glow.modifiers, &ph) {
                effects.glow = Some(QuickGlow {
                    radius: glow.radius_emu / 914400.0,
                    color,
                    opacity: glow.alpha.clamp(0.0, 1.0),
                });
            }
        }
        if let Some(edge) = &style.soft_edge {
            effects.soft_edge = Some(QuickSoftEdge {
                radius: edge.radius_emu / 914400.0,
            });
        }
        if let Some(reflection) = &style.reflection {
            effects.reflection = Some(QuickReflection {
                blur: reflection.blur_emu / 914400.0,
                distance: reflection.dist_emu / 914400.0,
                opacity: reflection.alpha.clamp(0.0, 1.0),
            });
        }
        Some(effects)
    }

    pub fn color_of(&self, cell: &Cell) -> Option<String> {
        let value = cell.value.as_deref();
        if let Some(hex) = value.and_then(normalize_hex) {
            return Some(hex);
        }
        // A document without a theme part has nothing for THEMEVAL() to read; Visio shows the
        // cached colour index (0 = black) instead of an invented accent colour.
        if !self.present {
            let themed = cell
                .formula
                .as_deref()
                .is_some_and(|formula| formula.to_ascii_uppercase().contains("THEME"));
            if let (true, Some(value)) = (themed, value) {
                if !value.is_empty() && value.chars().all(|c| c.is_ascii_digit()) {
                    if let Some(color) = self.colors.get(value).cloned().or_else(|| {
                        value
                            .parse::<usize>()
                            .ok()
                            .and_then(|n| BUILTIN_COLORS.get(n).map(|s| s.to_string()))
                    }) {
                        return Some(color);
                    }
                }
            }
        }
        if let Some(formula) = cell.formula.as_deref() {
            if let Some(color) = self.eval_color(formula) {
                return Some(color);
            }
        }
        if value == Some("Themed") {
            return self.slot("accent1", true);
        }
        let value = value?;
        if value.chars().all(|c| c.is_ascii_digit()) {
            return self.colors.get(value).cloned().or_else(|| {
                value
                    .parse::<usize>()
                    .ok()
                    .and_then(|n| BUILTIN_COLORS.get(n).map(|s| s.to_string()))
            });
        }
        None
    }

    /// Role-aware resolution for a bare `THEMEVAL()`/`Themed` cell.
    pub fn themed_color(&self, cell: Option<&Cell>, role: &str) -> Option<String> {
        let cell = cell?;
        let value = cell.value.as_deref();
        let formula = cell.formula.as_deref().unwrap_or("");
        if value == Some("Themed")
            && (formula.is_empty() || is_bare_themeval(formula) || formula == "Inh")
        {
            return match role {
                "fill" | "fill_style" => self.themed_fill(),
                "line" => self.themed_line(),
                "text" => self.themed_text(false),
                _ => self.color_of(cell),
            };
        }
        self.color_of(cell)
    }

    fn eval_color(&self, formula: &str) -> Option<String> {
        let expr = formula.trim();
        if expr.is_empty() || expr.eq_ignore_ascii_case("inh") {
            return None;
        }
        if let Some(hex) = normalize_hex(expr) {
            return Some(hex);
        }
        if let Some(hex) = normalize_hex(expr.trim_matches('"')) {
            return Some(hex);
        }
        let (name, args) = split_call(expr)?;
        match name.to_ascii_uppercase().as_str() {
            "GUARD" | "THEMEGUARD" => self.eval_color(args.first()?),
            "THEMEVAL" => {
                let key = args.first().map(|arg| unquote(arg)).unwrap_or_default();
                if key.is_empty() {
                    return self.slot("accent1", true);
                }
                if let Some(color) = self.slot(&key, false) {
                    return Some(color);
                }
                args.get(1).and_then(|arg| self.eval_color(arg))
            }
            "MSOTINT" | "TINT" => {
                let color = self.eval_color(args.first()?)?;
                let amount = args.get(1).and_then(|arg| arg.trim().parse::<f64>().ok())?;
                Some(tint(&color, amount))
            }
            "RGB" => {
                let mut channels = [0.0; 3];
                for (index, channel) in channels.iter_mut().enumerate() {
                    *channel = args.get(index)?.trim().parse::<f64>().ok()?;
                }
                Some(to_hex(channels[0], channels[1], channels[2]))
            }
            _ => self.slot(&name, false),
        }
    }
}

fn office_theme() -> Theme {
    office_theme_with_colors(BTreeMap::new())
}

fn office_theme_with_colors(colors: BTreeMap<String, String>) -> Theme {
    let mut slots = BTreeMap::new();
    for (key, value) in [
        ("dk1", "#000000"),
        ("lt1", "#ffffff"),
        ("dk2", "#44546a"),
        ("lt2", "#e7e6e6"),
        ("accent1", "#4472c4"),
        ("accent2", "#ed7d31"),
        ("accent3", "#a5a5a5"),
        ("accent4", "#ffc000"),
        ("accent5", "#5b9bd5"),
        ("accent6", "#70ad47"),
        ("hlink", "#0563c1"),
        ("folhlink", "#954f72"),
        ("bkgnd", "#ffffff"),
    ] {
        slots.insert(key.to_string(), value.to_string());
    }
    let variations = default_variations();
    for (key, value) in &variations[0] {
        slots.insert(key.clone(), value.clone());
    }
    Theme {
        legacy_shadow_offset: None,
        colors,
        slots,
        variations,
        active_variation: 0,
        name: "Office".to_string(),
        scheme_enum: None,
        line_weights_pt: Vec::new(),
        fill_styles: Vec::new(),
        line_styles: Vec::new(),
        connector_fill_styles: Vec::new(),
        connector_line_styles: Vec::new(),
        connector_line_weights_pt: Vec::new(),
        effect_styles: Vec::new(),
        variation_style_schemes: Vec::new(),
        embellishment: 3.0,
        minor_font: None,
        major_font: None,
        present: false,
    }
}

fn default_variations() -> Vec<BTreeMap<String, String>> {
    let accents = [
        "#4472c4", "#ed7d31", "#a5a5a5", "#ffc000", "#5b9bd5", "#70ad47",
    ];
    (0..4)
        .map(|shift| {
            let mut out = BTreeMap::new();
            for index in 0..6 {
                out.insert(
                    format!("variant{}", index + 1),
                    accents[(index + shift) % 6].to_string(),
                );
            }
            out
        })
        .collect()
}

/// One `fillStyleLst`/`lnStyleLst` entry: a base colour plus tint/shade,
/// optionally a gradient fill, line width and dash pattern.
fn read_theme_style(node: Node<'_, '_>) -> Option<ThemeStyle> {
    let (color, modifiers, _) = read_color_ref(node)?;
    let gradient = read_theme_gradient(node);
    let width_pt = node
        .attribute("w")
        .and_then(|width| width.parse::<f64>().ok())
        .map(|width| width / 12700.0);
    let dash = node
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "prstDash")
        .and_then(|n| n.attribute("val"))
        .map(str::to_string);
    Some(ThemeStyle {
        color,
        modifiers,
        gradient,
        width_pt,
        dash,
    })
}

/// Read the first colour reference in a theme style/effect node, including
/// tint/shade/lum modifiers and an `alpha` child (1000ths of a percent).
fn read_color_ref(node: Node<'_, '_>) -> Option<ThemeColorRef> {
    let color_node = node
        .descendants()
        .find(|n| n.is_element() && matches!(n.tag_name().name(), "schemeClr" | "srgbClr"))?;
    let color = match color_node.tag_name().name() {
        "schemeClr" => color_node.attribute("val")?.to_string(),
        _ => format!("#{}", color_node.attribute("val")?),
    };
    let mut modifiers = Vec::new();
    let mut alpha = 1.0;
    for child in color_node.children().filter(|n| n.is_element()) {
        let name = child.tag_name().name();
        let Some(value) = child.attribute("val").and_then(|v| v.parse::<f64>().ok()) else {
            continue;
        };
        match name {
            "alpha" => alpha = (value / 100000.0).clamp(0.0, 1.0),
            "tint" | "shade" | "lumMod" | "lumOff" | "satMod" | "satOff" => {
                modifiers.push((name.to_string(), value));
            }
            _ => {}
        }
    }
    Some((color, modifiers, alpha))
}

fn read_theme_gradient(node: Node<'_, '_>) -> Option<ThemeGradient> {
    let gradient = node
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "gradFill")?;
    let mut stops = Vec::new();
    for stop in gradient
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "gs")
    {
        let Some((color, modifiers, opacity)) = read_color_ref(stop) else {
            continue;
        };
        let position = stop
            .attribute("pos")
            .and_then(|value| value.parse::<f64>().ok())
            .unwrap_or(0.0)
            / 100000.0;
        stops.push(ThemeGradientStop {
            color,
            modifiers,
            position: position.clamp(0.0, 1.0),
            opacity,
        });
    }
    if stops.len() < 2 {
        return None;
    }
    stops.sort_by(|a, b| {
        a.position
            .partial_cmp(&b.position)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let angle = gradient
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "lin")
        .and_then(|n| n.attribute("ang"))
        .and_then(|value| value.parse::<f64>().ok())
        .map(|value| value / 60000.0 * std::f64::consts::PI / 180.0)
        .unwrap_or(0.0);
    Some(ThemeGradient { stops, angle })
}

fn read_effect_style(node: Node<'_, '_>) -> Option<ThemeEffectStyle> {
    let mut effect = ThemeEffectStyle::default();
    if let Some(shadow) = node
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "outerShdw")
    {
        if let Some((color, modifiers, alpha)) = read_color_ref(shadow) {
            effect.shadow = Some(ThemeShadowStyle {
                dist_emu: shadow
                    .attribute("dist")
                    .and_then(|value| value.parse::<f64>().ok())
                    .unwrap_or(0.0),
                dir: shadow
                    .attribute("dir")
                    .and_then(|value| value.parse::<f64>().ok())
                    .unwrap_or(0.0),
                blur_emu: shadow
                    .attribute("blurRad")
                    .and_then(|value| value.parse::<f64>().ok())
                    .unwrap_or(0.0),
                color,
                modifiers,
                alpha,
            });
        }
    }
    if let Some(glow) = node
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "glow")
    {
        if let Some((color, modifiers, alpha)) = read_color_ref(glow) {
            effect.glow = Some(ThemeGlowStyle {
                radius_emu: glow
                    .attribute("rad")
                    .and_then(|value| value.parse::<f64>().ok())
                    .unwrap_or(0.0),
                color,
                modifiers,
                alpha,
            });
        }
    }
    if let Some(edge) = node
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "softEdge")
    {
        effect.soft_edge = Some(ThemeSoftEdgeStyle {
            radius_emu: edge
                .attribute("rad")
                .and_then(|value| value.parse::<f64>().ok())
                .unwrap_or(0.0),
        });
    }
    if let Some(reflection) = node
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "reflection")
    {
        effect.reflection = Some(ThemeReflectionStyle {
            blur_emu: reflection
                .attribute("blurRad")
                .and_then(|value| value.parse::<f64>().ok())
                .unwrap_or(0.0),
            dist_emu: reflection
                .attribute("dist")
                .and_then(|value| value.parse::<f64>().ok())
                .unwrap_or(0.0),
            alpha: reflection
                .attribute("stA")
                .and_then(|value| value.parse::<f64>().ok())
                .map(|value| value / 100000.0)
                .unwrap_or(0.4),
        });
    }
    Some(effect)
}

fn read_variation_style_schemes(doc: &roxmltree::Document<'_>) -> Vec<VariationStyleScheme> {
    doc.descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "variationStyleScheme")
        .map(|scheme| {
            let list = |name: &str| {
                scheme
                    .children()
                    .find(|n| n.is_element() && n.tag_name().name() == name)
            };
            VariationStyleScheme {
                variants: scheme
                    .children()
                    .filter(|n| n.is_element() && n.tag_name().name() == "varStyle")
                    .map(|variant| VariationStyle {
                        fill_idx: variant
                            .attribute("fillIdx")
                            .and_then(|value| value.parse::<usize>().ok())
                            .unwrap_or(1),
                        line_idx: variant
                            .attribute("lineIdx")
                            .and_then(|value| value.parse::<usize>().ok())
                            .unwrap_or(1),
                        effect_idx: variant
                            .attribute("effectIdx")
                            .and_then(|value| value.parse::<usize>().ok())
                            .unwrap_or(1),
                    })
                    .collect(),
                fill_styles: list("fillStyleLst")
                    .map(|list| {
                        list.children()
                            .filter(|n| n.is_element())
                            .filter_map(read_theme_style)
                            .collect()
                    })
                    .unwrap_or_default(),
                line_styles: list("lnStyleLst")
                    .map(|list| {
                        list.children()
                            .filter(|n| n.is_element() && n.tag_name().name() == "ln")
                            .filter_map(read_theme_style)
                            .collect()
                    })
                    .unwrap_or_default(),
            }
        })
        .collect()
}

fn read_scheme(doc: &roxmltree::Document<'_>) -> BTreeMap<String, String> {
    let mut slots = BTreeMap::new();
    let Some(scheme) = doc
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "clrScheme")
    else {
        return slots;
    };
    for entry in scheme.children().filter(|n| n.is_element()) {
        let Some(hex) = direct_theme_color(entry) else {
            continue;
        };
        slots.insert(canon(entry.tag_name().name()), hex);
    }
    slots
}

/// A scheme entry's own `srgbClr`/`sysClr` child. Nested `extLst` colours are
/// the Visio variation palette, not part of the base scheme.
fn direct_theme_color(entry: Node<'_, '_>) -> Option<String> {
    for child in entry.children().filter(|n| n.is_element()) {
        let hex = match child.tag_name().name() {
            "srgbClr" => child.attribute("val").and_then(normalize_hex),
            "sysClr" => child
                .attribute("lastClr")
                .or_else(|| child.attribute("val"))
                .and_then(normalize_hex),
            _ => None,
        };
        if hex.is_some() {
            return hex;
        }
    }
    None
}

fn first_theme_color(node: Node<'_, '_>) -> Option<String> {
    for color in node
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "srgbClr")
    {
        if let Some(hex) = color.attribute("val").and_then(normalize_hex) {
            return Some(hex);
        }
    }
    for color in node
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "sysClr")
    {
        if let Some(hex) = color
            .attribute("lastClr")
            .or_else(|| color.attribute("val"))
            .and_then(normalize_hex)
        {
            return Some(hex);
        }
    }
    None
}

fn explicit_fill_scores(package: &Package) -> BTreeMap<String, usize> {
    let neutral = [
        "#ffffff", "#000000", "#f2f2f2", "#fefefe", "#d0d0d0", "#e7e6e6", "#d9d9d9",
    ];
    let mut counts = BTreeMap::new();
    for name in package.names() {
        let lower = name.to_ascii_lowercase();
        let is_style_source = (lower.starts_with("visio/pages/")
            || lower.starts_with("visio/masters/"))
            && lower.ends_with(".xml");
        if !is_style_source {
            continue;
        }
        let Ok(doc) = package.xml(name) else {
            continue;
        };
        for cell in doc
            .descendants()
            .filter(|n| n.is_element() && n.tag_name().name() == "Cell")
        {
            if cell.attribute("N") != Some("FillForegnd") {
                continue;
            }
            let Some(hex) = cell.attribute("V").and_then(normalize_hex) else {
                continue;
            };
            if neutral.contains(&hex.as_str()) {
                continue;
            }
            *counts.entry(hex).or_insert(0) += 1;
        }
    }
    counts
}

pub fn is_concrete(cell: &Cell) -> bool {
    let Some(value) = cell.value.as_deref() else {
        return false;
    };
    if value.is_empty() || value == "Themed" {
        return false;
    }
    value.starts_with('#') || value.parse::<f64>().is_ok()
}

/// Mix a colour towards white (n > 0) or black (n < 0), as Visio's MSOTINT.
fn srgb_to_linear(channel: u8) -> f64 {
    let value = channel as f64 / 255.0;
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(value: f64) -> u8 {
    let value = value.clamp(0.0, 1.0);
    let channel = if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    };
    (channel * 255.0).round().clamp(0.0, 255.0) as u8
}

fn theme_tint(hex: &str, original_fraction: f64) -> String {
    let Some((r, g, b)) = hex_channels(hex) else {
        return hex.to_string();
    };
    let p = original_fraction.clamp(0.0, 1.0);
    let blend = |channel: u8| linear_to_srgb(p * srgb_to_linear(channel) + (1.0 - p));
    to_hex(blend(r) as f64, blend(g) as f64, blend(b) as f64)
}

fn theme_shade(hex: &str, factor: f64) -> String {
    let Some((r, g, b)) = hex_channels(hex) else {
        return hex.to_string();
    };
    let p = factor.clamp(0.0, 1.0);
    let blend = |channel: u8| linear_to_srgb(p * srgb_to_linear(channel));
    to_hex(blend(r) as f64, blend(g) as f64, blend(b) as f64)
}

/// `sat_mod` / `lum_mod` multiply and `lum_off` / `sat_off` add to the HSL saturation and luminance of the colour (0..1), the DrawingML meaning of satMod, lumMod, lumOff and satOff.
fn hsl_modify(hex: &str, sat_mod: f64, lum_mod: f64, lum_off: f64, sat_off: f64) -> String {
    let Some((r8, g8, b8)) = hex_channels(hex) else {
        return hex.to_string();
    };
    let (r, g, b) = (r8 as f64 / 255.0, g8 as f64 / 255.0, b8 as f64 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let mut l = (max + min) / 2.0;
    let d = max - min;
    let (h, mut s) = if d < 1e-12 {
        (0.0, 0.0)
    } else {
        let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
        let h = if max == r { ((g - b) / d).rem_euclid(6.0) } else if max == g { (b - r) / d + 2.0 } else { (r - g) / d + 4.0 };
        (h / 6.0, s)
    };
    s = (s * sat_mod + sat_off).clamp(0.0, 1.0);
    l = (l * lum_mod + lum_off).clamp(0.0, 1.0);
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let channel = |t: f64| {
        let t = t.rem_euclid(1.0);
        let v = if t < 1.0 / 6.0 { p + (q - p) * 6.0 * t } else if t < 0.5 { q } else if t < 2.0 / 3.0 { p + (q - p) * (2.0 / 3.0 - t) * 6.0 } else { p };
        v * 255.0
    };
    if s == 0.0 {
        return to_hex(l * 255.0, l * 255.0, l * 255.0);
    }
    to_hex(channel(h + 1.0 / 3.0), channel(h), channel(h - 1.0 / 3.0))
}

fn theme_lum_off(hex: &str, offset: f64) -> String {
    let Some((r, g, b)) = hex_channels(hex) else {
        return hex.to_string();
    };
    let blend = |channel: u8| linear_to_srgb(srgb_to_linear(channel) + offset);
    to_hex(blend(r) as f64, blend(g) as f64, blend(b) as f64)
}

fn hex_channels(hex: &str) -> Option<(u8, u8, u8)> {
    if !hex.starts_with('#') || hex.len() != 7 {
        return None;
    }
    Some((
        u8::from_str_radix(&hex[1..3], 16).ok()?,
        u8::from_str_radix(&hex[3..5], 16).ok()?,
        u8::from_str_radix(&hex[5..7], 16).ok()?,
    ))
}

pub fn tint(hex: &str, n: f64) -> String {
    if !hex.starts_with('#') || hex.len() != 7 || !n.is_finite() {
        return hex.to_string();
    }
    let (Some(r), Some(g), Some(b)) = (
        u8::from_str_radix(&hex[1..3], 16).ok(),
        u8::from_str_radix(&hex[3..5], 16).ok(),
        u8::from_str_radix(&hex[5..7], 16).ok(),
    ) else {
        return hex.to_string();
    };
    let amount = n.abs() / 100.0;
    let target = if n >= 0.0 { 255.0 } else { 0.0 };
    to_hex(
        r as f64 + (target - r as f64) * amount,
        g as f64 + (target - g as f64) * amount,
        b as f64 + (target - b as f64) * amount,
    )
}

fn to_hex(r: f64, g: f64, b: f64) -> String {
    let channel = |value: f64| value.round().clamp(0.0, 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}", channel(r), channel(g), channel(b))
}

/// `#rrggbb` (or bare `rrggbb`) lower-cased; `None` for anything else.
pub fn normalize_hex_color(value: &str) -> Option<String> {
    normalize_hex(value)
}

fn normalize_hex(value: &str) -> Option<String> {
    let value = value.strip_prefix('#').unwrap_or(value);
    if value.len() == 6 && value.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(format!("#{}", value.to_ascii_lowercase()))
    } else {
        None
    }
}

fn canon(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn unquote(value: &str) -> String {
    value.trim().trim_matches('"').to_string()
}

fn is_bare_themeval(formula: &str) -> bool {
    formula
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .eq_ignore_ascii_case("themeval()")
}

/// True for a cell whose value is the theme-placeholder `Themed` with no
/// meaningful formula of its own. Its concrete colour is decided by the
/// theme role, not by a QuickStyle colour index.
pub fn is_bare_themed(cell: &Cell) -> bool {
    if cell.value.as_deref() != Some("Themed") {
        return false;
    }
    let formula = cell.formula.as_deref().unwrap_or("");
    formula.is_empty() || is_bare_themeval(formula) || formula == "Inh"
}

/// Split a simple `NAME(arg, ...)` expression, respecting nesting and quotes.
fn split_call(expr: &str) -> Option<(String, Vec<String>)> {
    let open = expr.find('(')?;
    if !expr.ends_with(')') {
        return None;
    }
    let name = expr[..open].trim();
    if name.is_empty() {
        return None;
    }
    let inner = &expr[open + 1..expr.len() - 1];
    let mut args = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    let mut quoted = false;
    for c in inner.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                current.push(c);
            }
            '(' if !quoted => {
                depth += 1;
                current.push(c);
            }
            ')' if !quoted => {
                depth = depth.saturating_sub(1);
                current.push(c);
            }
            ',' if !quoted && depth == 0 => {
                args.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(c),
        }
    }
    if !current.trim().is_empty() || !args.is_empty() {
        args.push(current.trim().to_string());
    }
    Some((name.to_string(), args))
}

#[cfg(test)]
mod hsl_tests {
    use super::hsl_modify;

    #[test]
    fn satmod_99_percent_turns_retrospect_accent_into_the_rgb_visio_reports() {
        // Visio (variants-compare.mjs, Retrospect variant 0): FillForegnd RGB(188; 88; 45) for the fill style "satMod 99 percent, lumMod 100 percent" on #BD582C
        assert_eq!(hsl_modify("#bd582c", 0.99, 1.0, 0.0, 0.0), "#bc582d");
    }

    #[test]
    fn lummod_scales_the_hsl_luminance() {
        assert_eq!(hsl_modify("#ff0000", 1.0, 0.5, 0.0, 0.0), "#800000");
        assert_eq!(hsl_modify("#808080", 1.0, 1.0, 0.0, 0.0), "#808080");
    }

    #[test]
    fn lumoff_and_satoff_add_and_the_results_are_clamped() {
        assert_eq!(hsl_modify("#336699", 1.0, 1.0, 1.0, 0.0), "#ffffff");
        assert_eq!(hsl_modify("#336699", 0.0, 1.0, 0.0, 0.0), "#666666");
        assert_eq!(hsl_modify("#336699", 1.0, 0.0, 0.0, 0.0), "#000000");
        // a grey stays grey under satMod; satOff gives it colour only through its hue, which a grey has none of (0 = red)
        assert_eq!(hsl_modify("#808080", 5.0, 1.0, 0.0, 0.0), "#808080");
    }
}
