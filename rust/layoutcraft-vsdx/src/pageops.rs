//! Page operations on a package: add, delete, rename, reorder and duplicate pages.
//!
//! A drawing's pages are listed in `visio/pages/pages.xml` (a `Page` element per page, in display order, each
//! with a unique `ID`, a `NameU` / `Name` and a `Rel` to its part through `pages.xml.rels`); every page has its
//! own part (`pageN.xml`, content type `application/vnd.ms-visio.page+xml`) and perhaps its own relationships
//! (to the masters and pictures it uses). `windows.xml` remembers the current page by its `ID`. Visio (probe-draw-and-pages.ps1) numbers the IDs arbitrarily (0, 4, 6, 5 after adding two pages and duplicating one) and takes the order from
//! the order of the elements, so the part names and IDs here only have to be unique.
use std::collections::BTreeMap;

use crate::writer::{attr, escape_attr, set_attr, write_package_edit};
use crate::{Error, Package, Result};

const PAGES: &str = "visio/pages/pages.xml";
const PAGES_RELS: &str = "visio/pages/_rels/pages.xml.rels";
const CONTENT_TYPES: &str = "[Content_Types].xml";
const REL_PAGE: &str = "http://schemas.microsoft.com/visio/2010/relationships/page";
const PAGE_CONTENT_TYPE: &str = "application/vnd.ms-visio.page+xml";
const EMPTY_PAGE: &str = "<?xml version='1.0' encoding='utf-8' ?><PageContents xmlns='http://schemas.microsoft.com/office/visio/2012/main' xmlns:r='http://schemas.openxmlformats.org/officeDocument/2006/relationships' xml:space='preserve'><Shapes></Shapes></PageContents>";

/// One `Page` element of `pages.xml`.
#[derive(Debug, Clone)]
struct PageEntry {
    start: usize,
    end: usize,
    id: String,
    name: String,
    rel: String,
    /// `Background='1'`: a background page (the shell lists only the other pages).
    background: bool,
    /// `BackPage`: the ID of the background page shown beneath this one.
    back: Option<String>,
}

fn entries(xml: &str) -> Vec<PageEntry> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(relative) = xml[at..].find("<Page ") {
        let start = at + relative;
        let Some(open_close) = xml[start..].find('>') else { break };
        let open_end = start + open_close + 1;
        let end = if xml[start..open_end].ends_with("/>") {
            open_end
        } else {
            match xml[open_end..].find("</Page>") {
                Some(n) => open_end + n + "</Page>".len(),
                None => break,
            }
        };
        let tag = &xml[start..open_end];
        let element = &xml[start..end];
        let rel = element
            .find("<Rel ")
            .and_then(|r| element[r..].find('>').map(|c| attr(&element[r..r + c + 1], "r:id")))
            .flatten()
            .unwrap_or_default();
        out.push(PageEntry {
            start,
            end,
            id: attr(tag, "ID").unwrap_or_default(),
            name: attr(tag, "NameU").or_else(|| attr(tag, "Name")).unwrap_or_default(),
            rel,
            background: attr(tag, "Background").as_deref() == Some("1"),
            back: attr(tag, "BackPage"),
        });
        at = end;
    }
    out
}

/// A page name Visio accepts: 1 to 31 characters, none of `< > & ' "` or control characters.
pub fn clean_page_name(name: &str) -> Result<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(Error("a page needs a name".to_string()));
    }
    if trimmed.chars().count() > 31 {
        return Err(Error("a page name can have at most 31 characters".to_string()));
    }
    if trimmed.chars().any(|c| c.is_control() || "<>&'\"".contains(c)) {
        return Err(Error("a page name cannot contain < > & ' \" or control characters".to_string()));
    }
    Ok(trimmed.to_string())
}

struct Pages {
    xml: String,
    rels: String,
    list: Vec<PageEntry>,
    /// Positions in `list` of the pages the shell shows (the foreground pages, in order; all pages when there is no
    /// foreground page at all, as the importer does): the page index of every operation counts only these.
    shown: Vec<usize>,
}

impl Pages {
    fn load(package: &Package) -> Result<Self> {
        let xml = package.text(PAGES)?.to_string();
        let rels = if package.contains(PAGES_RELS) { package.text(PAGES_RELS)?.to_string() } else { String::new() };
        let list = entries(&xml);
        if list.is_empty() {
            return Err(Error("the drawing has no pages".to_string()));
        }
        let has_foreground = list.iter().any(|p| !p.background);
        let shown = (0..list.len()).filter(|&i| !has_foreground || !list[i].background).collect();
        Ok(Self { xml, rels, list, shown })
    }

    fn get(&self, index: usize) -> Result<&PageEntry> {
        self.shown.get(index).map(|&i| &self.list[i]).ok_or_else(|| Error(format!("page {index} does not exist")))
    }

    /// Position in `list` of shown page `index`.
    fn slot(&self, index: usize) -> Result<usize> {
        self.shown.get(index).copied().ok_or_else(|| Error(format!("page {index} does not exist")))
    }

    /// A page name that is not taken (`base`, else `base (2)`, ...), cut to 31 characters.
    fn free_name(&self, base: &str) -> String {
        let taken = |name: &str| self.list.iter().any(|p| p.name.eq_ignore_ascii_case(name));
        let cut = |s: &str, n: usize| s.chars().take(n).collect::<String>();
        if !taken(base) {
            return cut(base, 31);
        }
        for n in 2.. {
            let suffix = format!(" ({n})");
            let candidate = format!("{}{}", cut(base, 31 - suffix.chars().count()), suffix);
            if !taken(&candidate) {
                return candidate;
            }
        }
        unreachable!()
    }

    fn next_id(&self) -> u64 {
        self.list.iter().filter_map(|p| p.id.parse::<u64>().ok()).max().map_or(0, |m| m + 1)
    }

    fn next_rel(&self) -> String {
        let mut max = 0u64;
        let mut at = 0;
        while let Some(r) = self.rels[at..].find("Id=") {
            let value = &self.rels[at + r + 3..];
            let digits: String = value.trim_start_matches(['"', '\'']).strip_prefix("rId").unwrap_or("").chars().take_while(|c| c.is_ascii_digit()).collect();
            max = max.max(digits.parse::<u64>().unwrap_or(0));
            at += r + 3;
        }
        format!("rId{}", max + 1)
    }

    /// The part a relationship id of `pages.xml` points to (`visio/pages/pageN.xml`).
    fn part_of(&self, package: &Package, rel: &str) -> Result<String> {
        package
            .relationships(PAGES)?
            .get(rel)
            .map(|r| r.target.clone())
            .ok_or_else(|| Error(format!("pages.xml has no relationship {rel}")))
    }
}

fn rels_part_of(part: &str) -> String {
    let (dir, file) = part.rsplit_once('/').unwrap_or(("", part));
    format!("{dir}/_rels/{file}.rels")
}

fn next_part_number(package: &Package, extra: &[String]) -> u64 {
    let mut max = 0u64;
    for name in package.names().map(str::to_string).chain(extra.iter().cloned()) {
        if let Some(n) = name.strip_prefix("visio/pages/page").and_then(|r| r.strip_suffix(".xml")).and_then(|n| n.parse::<u64>().ok()) {
            max = max.max(n);
        }
    }
    max + 1
}

fn add_override(content_types: &mut String, part: &str) -> Result<()> {
    let at = content_types.rfind("</Types>").ok_or_else(|| Error("bad [Content_Types].xml".to_string()))?;
    content_types.insert_str(at, &format!("<Override PartName='/{part}' ContentType='{PAGE_CONTENT_TYPE}'/>"));
    Ok(())
}

/// Add an empty page after page `after` with the page sheet (size, scale, units) of that page. `name` None:
/// "Page-N" for the first free N. Returns the new package bytes and the new page's index.
pub fn add_page(package: &Package, after: usize, name: Option<&str>) -> Result<(Vec<u8>, usize)> {
    let pages = Pages::load(package)?;
    let reference = pages.get(after)?.clone();
    let name = match name {
        Some(n) => {
            let n = clean_page_name(n)?;
            if pages.list.iter().any(|p| p.name.eq_ignore_ascii_case(&n)) {
                return Err(Error(format!("a page called {n} exists already")));
            }
            n
        }
        None => {
            let mut n = pages.list.len() + 1;
            loop {
                let candidate = format!("Page-{n}");
                if !pages.list.iter().any(|p| p.name.eq_ignore_ascii_case(&candidate)) {
                    break candidate;
                }
                n += 1;
            }
        }
    };
    let part_no = next_part_number(package, &[]);
    let part = format!("visio/pages/page{part_no}.xml");
    let rel = pages.next_rel();
    // The page sheet of the reference page, so the new page has the same size and scale.
    let element = &pages.xml[reference.start..reference.end];
    let sheet = element
        .find("<PageSheet")
        .and_then(|s| element[s..].find("</PageSheet>").map(|e| element[s..s + e + "</PageSheet>".len()].to_string()))
        .unwrap_or_default();
    let open_end = element.find('>').map_or(0, |n| n + 1);
    let mut open = set_attr(&element[..open_end], "ID", &pages.next_id().to_string());
    open = set_attr(&open, "NameU", &name);
    open = set_attr(&open, "Name", &name);
    let new_element = format!("{open}{sheet}<Rel r:id='{rel}'/></Page>");
    let mut xml = pages.xml.clone();
    xml.insert_str(reference.end, &new_element);
    let mut rels = pages.rels.clone();
    let at = rels.rfind("</Relationships>").ok_or_else(|| Error("bad pages.xml.rels".to_string()))?;
    rels.insert_str(at, &format!("<Relationship Id='{rel}' Type='{REL_PAGE}' Target='page{part_no}.xml'/>"));
    let mut content_types = package.text(CONTENT_TYPES)?.to_string();
    add_override(&mut content_types, &part)?;
    let mut extras: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    extras.insert(PAGES.to_string(), xml.into_bytes());
    extras.insert(PAGES_RELS.to_string(), rels.into_bytes());
    extras.insert(part, EMPTY_PAGE.as_bytes().to_vec());
    extras.insert(CONTENT_TYPES.to_string(), content_types.into_bytes());
    Ok((write_package_edit(package, extras, &[])?, after + 1))
}

/// Remove page `index` with its part, its relationships and its content type. The last page cannot go.
pub fn delete_page(package: &Package, index: usize) -> Result<Vec<u8>> {
    let pages = Pages::load(package)?;
    if pages.shown.len() == 1 {
        return Err(Error("a drawing needs at least one page".to_string()));
    }
    let gone = pages.get(index)?.clone();
    let part = pages.part_of(package, &gone.rel)?;
    let mut xml = pages.xml.clone();
    xml.replace_range(gone.start..gone.end, "");
    // Pages that used the deleted one as their background lose the reference.
    for key in ["BackPage='", "BackPage=\""] {
        let pattern = format!("{key}{}", gone.id);
        xml = xml.replace(&format!(" {pattern}'"), "").replace(&format!(" {pattern}\""), "");
    }
    let mut rels = pages.rels.clone();
    if let Some(at) = rels.find(&format!("Id=\"{}\"", gone.rel)).or_else(|| rels.find(&format!("Id='{}'", gone.rel))) {
        let start = rels[..at].rfind("<Relationship ").unwrap_or(at);
        let end = rels[at..].find("/>").map_or(rels.len(), |n| at + n + 2);
        rels.replace_range(start..end, "");
    }
    let mut content_types = package.text(CONTENT_TYPES)?.to_string();
    for quote in ['\'', '"'] {
        let marker = format!("PartName={quote}/{part}{quote}");
        if let Some(at) = content_types.find(&marker) {
            let start = content_types[..at].rfind("<Override ").unwrap_or(at);
            let end = content_types[at..].find("/>").map_or(content_types.len(), |n| at + n + 2);
            content_types.replace_range(start..end, "");
        }
    }
    let mut extras: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    extras.insert(PAGES.to_string(), xml.into_bytes());
    extras.insert(PAGES_RELS.to_string(), rels.into_bytes());
    extras.insert(CONTENT_TYPES.to_string(), content_types.into_bytes());
    // The window shows another page when it showed the deleted one.
    if package.contains("visio/windows.xml") {
        let windows = package.text("visio/windows.xml")?;
        if attr_in(windows, "Page").as_deref() == Some(gone.id.as_str()) {
            let fallback = pages.list.iter().find(|p| p.id != gone.id && !p.background).map(|p| p.id.clone()).unwrap_or_else(|| "0".to_string());
            extras.insert("visio/windows.xml".to_string(), windows.replace(&format!("Page='{}'", gone.id), &format!("Page='{fallback}'")).replace(&format!("Page=\"{}\"", gone.id), &format!("Page=\"{fallback}\"")).into_bytes());
        }
    }
    let removed = vec![part.clone(), rels_part_of(&part)];
    write_package_edit(package, extras, &removed)
}

/// The value of attribute `name` in the first element of `text` that has it (for `Page=` in windows.xml).
fn attr_in(text: &str, name: &str) -> Option<String> {
    for quote in ['\'', '"'] {
        let pattern = format!(" {name}={quote}");
        if let Some(at) = text.find(&pattern) {
            let rest = &text[at + pattern.len()..];
            return rest.find(quote).map(|e| rest[..e].to_string());
        }
    }
    None
}

/// Rename page `index`. Names are unique (case-insensitively) and at most 31 characters.
pub fn rename_page(package: &Package, index: usize, name: &str) -> Result<Vec<u8>> {
    let pages = Pages::load(package)?;
    let page = pages.get(index)?.clone();
    let name = clean_page_name(name)?;
    if pages.list.iter().any(|p| p.id != page.id && p.name.eq_ignore_ascii_case(&name)) {
        return Err(Error(format!("a page called {name} exists already")));
    }
    let open_end = pages.xml[page.start..].find('>').map_or(0, |n| page.start + n + 1);
    let mut open = pages.xml[page.start..open_end].to_string();
    open = set_attr(&open, "NameU", &name);
    open = set_attr(&open, "Name", &name);
    for flag in ["IsCustomNameU", "IsCustomName"] {
        if attr(&open, flag).is_none() {
            open = set_attr(&open, flag, "1");
        }
    }
    let mut xml = pages.xml.clone();
    xml.replace_range(page.start..open_end, &open);
    let mut extras: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    extras.insert(PAGES.to_string(), xml.into_bytes());
    write_package_edit(package, extras, &[])
}

/// Move page `from` so that it ends up at position `to` (0-based, in the order after the move).
pub fn move_page(package: &Package, from: usize, to: usize) -> Result<Vec<u8>> {
    let pages = Pages::load(package)?;
    let (from, to) = (pages.slot(from)?, pages.slot(to)?);
    if from == to {
        return write_package_edit(package, BTreeMap::new(), &[]);
    }
    let mut elements: Vec<String> = pages.list.iter().map(|p| pages.xml[p.start..p.end].to_string()).collect();
    let moved = elements.remove(from);
    elements.insert(to, moved);
    // The text between and around the page elements stays: only the elements are put in a new order.
    let mut xml = String::new();
    let mut cursor = 0;
    for (slot, element) in pages.list.iter().zip(&elements) {
        xml.push_str(&pages.xml[cursor..slot.start]);
        xml.push_str(element);
        cursor = slot.end;
    }
    xml.push_str(&pages.xml[cursor..]);
    let mut extras: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    extras.insert(PAGES.to_string(), xml.into_bytes());
    write_package_edit(package, extras, &[])
}

/// Copy page `index` (its shapes, relationships and page sheet) to a new page right after it, called
/// "<name> (2)" (or the next free number). Returns the new package bytes and the new page's index.
pub fn duplicate_page(package: &Package, index: usize) -> Result<(Vec<u8>, usize)> {
    let pages = Pages::load(package)?;
    let original = pages.get(index)?.clone();
    let part = pages.part_of(package, &original.rel)?;
    let part_no = next_part_number(package, &[]);
    let new_part = format!("visio/pages/page{part_no}.xml");
    let rel = pages.next_rel();
    let name = pages.free_name(&original.name);
    let element = &pages.xml[original.start..original.end];
    let open_end = element.find('>').map_or(0, |n| n + 1);
    let mut open = set_attr(&element[..open_end], "ID", &pages.next_id().to_string());
    open = set_attr(&open, "NameU", &name);
    open = set_attr(&open, "Name", &name);
    let mut body = element[open_end..].to_string();
    if let Some(r) = body.find("<Rel ") {
        let close = r + body[r..].find('>').unwrap_or(0) + 1;
        let tag = body[r..close].to_string();
        body.replace_range(r..close, &set_attr(&tag, "r:id", &rel));
    }
    let mut xml = pages.xml.clone();
    xml.insert_str(original.end, &format!("{open}{body}"));
    let mut rels = pages.rels.clone();
    let at = rels.rfind("</Relationships>").ok_or_else(|| Error("bad pages.xml.rels".to_string()))?;
    rels.insert_str(at, &format!("<Relationship Id='{rel}' Type='{REL_PAGE}' Target='page{part_no}.xml'/>"));
    let mut content_types = package.text(CONTENT_TYPES)?.to_string();
    add_override(&mut content_types, &new_part)?;
    let mut extras: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    extras.insert(PAGES.to_string(), xml.into_bytes());
    extras.insert(PAGES_RELS.to_string(), rels.into_bytes());
    extras.insert(CONTENT_TYPES.to_string(), content_types.into_bytes());
    extras.insert(new_part.clone(), package.entry(&part).ok_or_else(|| Error(format!("missing part {part}")))?.to_vec());
    // The copy relates to the same masters and pictures as the original (the targets are relative).
    let rels_part = rels_part_of(&part);
    if package.contains(&rels_part) {
        extras.insert(rels_part_of(&new_part), package.entry(&rels_part).unwrap_or(&[]).to_vec());
    }
    let _ = escape_attr;
    Ok((write_package_edit(package, extras, &[])?, index + 1))
}

/// Smallest and largest page side the shell offers (inches), as for a new drawing.
pub const MIN_PAGE_SIDE: f64 = 0.5;
pub const MAX_PAGE_SIDE: f64 = 200.0;

/// Set the size of page `index` (`PageWidth` / `PageHeight` of its page sheet, inches). The shapes keep their positions: the
/// page's origin is its lower left corner, so a taller page shows more above the shapes, a shorter one cuts the top.
pub fn set_page_size(package: &Package, index: usize, width: f64, height: f64) -> Result<Vec<u8>> {
    for (name, value) in [("width", width), ("height", height)] {
        if !value.is_finite() || !(MIN_PAGE_SIDE..=MAX_PAGE_SIDE).contains(&value) {
            return Err(Error(format!("the page {name} must be between {MIN_PAGE_SIDE} and {MAX_PAGE_SIDE} inches (about 13 mm and 5 m)")));
        }
    }
    let pages = Pages::load(package)?;
    let page = pages.get(index)?.clone();
    let mut xml = pages.xml.clone();
    let mut entry = xml[page.start..page.end].to_string();
    let sheet_open = entry.find("<PageSheet").ok_or_else(|| Error("the page has no page sheet".to_string()))?;
    let sheet_end = entry[sheet_open..].find('>').map(|n| sheet_open + n + 1).ok_or_else(|| Error("bad page sheet".to_string()))?;
    for (name, value) in [("PageWidth", width), ("PageHeight", height)] {
        let text = format!("{}", (value * 1e9).round() / 1e9);
        let key = format!("<Cell N='{name}'");
        match entry.find(&key) {
            Some(at) => {
                let end = entry[at..].find('>').map_or(entry.len(), |n| at + n + 1);
                let mut tag = entry[at..end].to_string();
                tag = set_attr(&tag, "V", &text);
                if attr(&tag, "U").is_none() {
                    tag = tag.replacen(&format!("<Cell N='{name}'"), &format!("<Cell N='{name}' U='IN'"), 1);
                }
                // a formula would overwrite the value when Visio recalculates
                if let Some(f) = tag.find(" F='") {
                    if let Some(close) = tag[f + 4..].find('\'') {
                        tag.replace_range(f..f + 4 + close + 1, "");
                    }
                }
                entry.replace_range(at..end, &tag);
            }
            None => entry.insert_str(sheet_end, &format!("<Cell N='{name}' V='{text}' U='IN'/>")),
        }
    }
    xml.replace_range(page.start..page.end, &entry);
    let mut extras: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    extras.insert(PAGES.to_string(), xml.into_bytes());
    write_package_edit(package, extras, &[])
}

/// The `<Section N='Layer'>` of page `index`'s page sheet as a byte range inside the whole `pages.xml` text (created, empty, at
/// the end of the page sheet when the page has none). Returns the text, the range of the section and the page's entry range.
fn layer_section(pages: &Pages, index: usize) -> Result<(String, usize, usize)> {
    let page = pages.get(index)?.clone();
    let mut xml = pages.xml.clone();
    let entry = &xml[page.start..page.end];
    let sheet_close = entry.find("</PageSheet>").ok_or_else(|| Error("the page has no page sheet".to_string()))? + page.start;
    let found = xml[page.start..sheet_close].find("<Section N='Layer'").map(|n| n + page.start);
    let start = match found {
        Some(at) => at,
        None => {
            xml.insert_str(sheet_close, "<Section N='Layer'></Section>");
            sheet_close
        }
    };
    let end = xml[start..].find("</Section>").map(|n| start + n + "</Section>".len()).ok_or_else(|| Error("bad layer section".to_string()))?;
    Ok((xml, start, end))
}

/// The rows of a layer section as `(IX, range)` inside `xml`.
fn layer_rows(xml: &str, start: usize, end: usize) -> Vec<(u32, usize, usize)> {
    let mut rows = Vec::new();
    let mut at = start;
    while let Some(relative) = xml[at..end].find("<Row ") {
        let open = at + relative;
        let close = xml[open..end].find("</Row>").map_or(end, |n| open + n + "</Row>".len());
        let ix = attr(&xml[open..close], "IX").and_then(|v| v.parse::<u32>().ok()).unwrap_or(rows.len() as u32);
        rows.push((ix, open, close));
        at = close;
    }
    rows
}

fn layer_name_of(row: &str) -> String {
    let key = "<Cell N='Name' V='";
    row.find(key).and_then(|a| row[a + key.len()..].find('\'').map(|e| row[a + key.len()..a + key.len() + e].to_string())).unwrap_or_default()
}

/// Add a layer to page `index` (the next free `IX`, the cells Visio writes: visible, printing, unlocked, snap and glue on).
/// Layer names are unique on a page (case-insensitively). Returns the package bytes and the new layer's index.
pub fn add_layer(package: &Package, index: usize, name: &str) -> Result<(Vec<u8>, u32)> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 64 || name.chars().any(|c| c.is_control() || "<>&'\"".contains(c)) {
        return Err(Error("a layer name is 1 to 64 characters without < > & or quotes".to_string()));
    }
    let pages = Pages::load(package)?;
    let (mut xml, start, end) = layer_section(&pages, index)?;
    let rows = layer_rows(&xml, start, end);
    if rows.iter().any(|(_, s, e)| layer_name_of(&xml[*s..*e]).eq_ignore_ascii_case(name)) {
        return Err(Error(format!("a layer called {name} exists already")));
    }
    let ix = rows.iter().map(|(i, _, _)| *i + 1).max().unwrap_or(0);
    let row = format!(
        "<Row IX='{ix}'><Cell N='Name' V='{name}'/><Cell N='Color' V='255'/><Cell N='Status' V='0'/><Cell N='Visible' V='1'/><Cell N='Print' V='1'/><Cell N='Active' V='0'/><Cell N='Lock' V='0'/><Cell N='Snap' V='1'/><Cell N='Glue' V='1'/><Cell N='NameUniv' V='{name}'/><Cell N='ColorTrans' V='0'/></Row>"
    );
    xml.insert_str(end - "</Section>".len(), &row);
    let mut extras: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    extras.insert(PAGES.to_string(), xml.into_bytes());
    Ok((write_package_edit(package, extras, &[])?, ix))
}

/// Change a layer of page `index`: its name and / or the flags visible, print, locked (the cells `Name` + `NameUniv`,
/// `Visible`, `Print`, `Lock`). A shape on a layer that is not visible is not drawn.
pub fn set_layer(package: &Package, index: usize, layer: u32, name: Option<&str>, visible: Option<bool>, print: Option<bool>, lock: Option<bool>) -> Result<Vec<u8>> {
    let pages = Pages::load(package)?;
    let (mut xml, start, end) = layer_section(&pages, index)?;
    let rows = layer_rows(&xml, start, end);
    let (_, rs, re) = *rows.iter().find(|(i, _, _)| *i == layer).ok_or_else(|| Error(format!("page {index} has no layer {layer}")))?;
    let mut row = xml[rs..re].to_string();
    if let Some(name) = name {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 64 || name.chars().any(|c| c.is_control() || "<>&'\"".contains(c)) {
            return Err(Error("a layer name is 1 to 64 characters without < > & or quotes".to_string()));
        }
        if rows.iter().any(|(i, s, e)| *i != layer && layer_name_of(&xml[*s..*e]).eq_ignore_ascii_case(name)) {
            return Err(Error(format!("a layer called {name} exists already")));
        }
        row = set_row_cell(&row, "Name", name);
        row = set_row_cell(&row, "NameUniv", name);
    }
    for (cell, value) in [("Visible", visible), ("Print", print), ("Lock", lock)] {
        if let Some(value) = value {
            row = set_row_cell(&row, cell, if value { "1" } else { "0" });
        }
    }
    xml.replace_range(rs..re, &row);
    let mut extras: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    extras.insert(PAGES.to_string(), xml.into_bytes());
    write_package_edit(package, extras, &[])
}

/// Set the colour of layer `layer` of page `index`: `#rrggbb` (written like Visio writes an RGB layer colour) or `none` (255, no
/// colour). Shapes on a coloured layer are drawn with lines and text in it and a white fill.
pub fn set_layer_color(package: &Package, index: usize, layer: u32, color: &str) -> Result<Vec<u8>> {
    let value = if color.eq_ignore_ascii_case("none") {
        "255".to_string()
    } else if color.len() == 7 && color.starts_with('#') && color[1..].chars().all(|c| c.is_ascii_hexdigit()) {
        color.to_ascii_lowercase()
    } else {
        return Err(Error("a layer colour is #rrggbb or none".to_string()));
    };
    let pages = Pages::load(package)?;
    let (mut xml, start, end) = layer_section(&pages, index)?;
    let rows = layer_rows(&xml, start, end);
    let (_, rs, re) = *rows.iter().find(|(i, _, _)| *i == layer).ok_or_else(|| Error(format!("page {index} has no layer {layer}")))?;
    let row = set_row_cell(&xml[rs..re], "Color", &value);
    xml.replace_range(rs..re, &row);
    let mut extras: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    extras.insert(PAGES.to_string(), xml.into_bytes());
    write_package_edit(package, extras, &[])
}

/// Set the value of cell `name` of a layer row (the cell is added when the row has none).
fn set_row_cell(row: &str, name: &str, value: &str) -> String {
    let key = format!("<Cell N='{name}'");
    match row.find(&key) {
        Some(at) => {
            let end = row[at..].find('>').map_or(row.len(), |n| at + n + 1);
            let mut out = row.to_string();
            out.replace_range(at..end, &format!("<Cell N='{name}' V='{value}'/>"));
            out
        }
        None => {
            let close = row.rfind("</Row>").unwrap_or(row.len());
            let mut out = row.to_string();
            out.insert_str(close, &format!("<Cell N='{name}' V='{value}'/>"));
            out
        }
    }
}

/// Delete layer `layer` of page `index`: its row goes, the layers after it move down one index (`IX`), and every shape of the page
/// drops the layer from its `LayerMember` cell (higher numbers are lowered), as Visio renumbers its layers. The shapes stay.
pub fn delete_layer(package: &Package, index: usize, layer: u32) -> Result<Vec<u8>> {
    let pages = Pages::load(package)?;
    let entry = pages.get(index)?.clone();
    let part = pages.part_of(package, &entry.rel)?;
    let (mut xml, start, end) = layer_section(&pages, index)?;
    let rows = layer_rows(&xml, start, end);
    if !rows.iter().any(|(i, _, _)| *i == layer) {
        return Err(Error(format!("page {index} has no layer {layer}")));
    }
    // back to front, so the ranges stay valid
    for (ix, rs, re) in rows.iter().rev() {
        if *ix == layer {
            xml.replace_range(*rs..*re, "");
        } else if *ix > layer {
            let row = xml[*rs..*re].to_string();
            let tag_end = row.find('>').map_or(0, |n| n + 1);
            let tag = set_attr(&row[..tag_end], "IX", &(*ix - 1).to_string());
            xml.replace_range(*rs..*rs + tag_end, &tag);
        }
    }
    let page_xml = package.text(&part)?.to_string();
    let key = "<Cell N='LayerMember'";
    let mut out = String::with_capacity(page_xml.len());
    let mut at = 0;
    while let Some(found) = page_xml[at..].find(key) {
        let open = at + found;
        let close = page_xml[open..].find('>').map_or(page_xml.len(), |n| open + n + 1);
        out.push_str(&page_xml[at..open]);
        let tag = &page_xml[open..close];
        match attr(tag, "V") {
            Some(value) if !value.trim().is_empty() => {
                let kept: Vec<String> = value
                    .split(';')
                    .filter_map(|n| n.trim().parse::<u32>().ok())
                    .filter(|n| *n != layer)
                    .map(|n| if n > layer { n - 1 } else { n })
                    .map(|n| n.to_string())
                    .collect();
                out.push_str(&format!("<Cell N='LayerMember' V='{}'/>", kept.join(";")));
            }
            _ => out.push_str(tag),
        }
        at = close;
    }
    out.push_str(&page_xml[at..]);
    let mut extras: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    extras.insert(PAGES.to_string(), xml.into_bytes());
    extras.insert(part, out.into_bytes());
    write_package_edit(package, extras, &[])
}

// ---- background pages ------------------------------------------------------------------------------------------------
// Visio (probe-background-page.ps1): a background page is a `Page` with `Background='1'`, and Visio writes the background
// pages FIRST in pages.xml; a page shows one beneath itself through `BackPage='<ID of the background page>'`. Clearing the
// assignment drops the attribute. Probed too: Visio hangs (a prompt) when a page that has a background of its own is made a
// background, and when a background that is still in use is turned back into a foreground page - both are avoided here
// (refused / the references are dropped first).

/// Background pages and the assignment of every shown page.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BackgroundInfo {
    /// (page ID, name) of every background page, in file order.
    pub backgrounds: Vec<(String, String)>,
    /// For every shown page, the ID of its background page.
    pub assigned: Vec<Option<String>>,
}

pub fn background_info(package: &Package) -> Result<BackgroundInfo> {
    let pages = Pages::load(package)?;
    let backgrounds = pages.list.iter().filter(|p| p.background).map(|p| (p.id.clone(), p.name.clone())).collect();
    let assigned = pages.shown.iter().map(|&i| pages.list[i].back.clone()).collect();
    Ok(BackgroundInfo { backgrounds, assigned })
}

/// `open` (a start tag) without attribute `name`.
fn remove_attr(open: &str, name: &str) -> String {
    for quote in ['\'', '"'] {
        let pattern = format!(" {name}={quote}");
        if let Some(at) = open.find(&pattern) {
            if let Some(end) = open[at + pattern.len()..].find(quote) {
                return format!("{}{}", &open[..at], &open[at + pattern.len() + end + 1..]);
            }
        }
    }
    open.to_string()
}

fn open_tag_range(xml: &str, start: usize) -> (usize, usize) {
    (start, xml[start..].find('>').map_or(start, |n| start + n + 1))
}

fn edit_pages_xml(package: &Package, xml: String, extra: Option<(String, Vec<u8>)>) -> Result<Vec<u8>> {
    let mut extras: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    extras.insert(PAGES.to_string(), xml.into_bytes());
    if let Some((part, bytes)) = extra {
        extras.insert(part, bytes);
    }
    write_package_edit(package, extras, &[])
}

/// Show background page `background` (its ID; `None` = no background) beneath shown page `index`.
pub fn set_back_page(package: &Package, index: usize, background: Option<&str>) -> Result<Vec<u8>> {
    let pages = Pages::load(package)?;
    let page = pages.get(index)?.clone();
    if let Some(id) = background {
        let target = pages.list.iter().find(|p| p.id == id).ok_or_else(|| Error(format!("there is no page with ID {id}")))?;
        if !target.background {
            return Err(Error(format!("page {} is not a background page", target.name)));
        }
        if target.id == page.id {
            return Err(Error("a page cannot be its own background".to_string()));
        }
    }
    let (a, b) = open_tag_range(&pages.xml, page.start);
    let mut open = remove_attr(&pages.xml[a..b], "BackPage");
    if let Some(id) = background {
        open = set_attr(&open, "BackPage", id);
    }
    let mut xml = pages.xml.clone();
    xml.replace_range(a..b, &open);
    edit_pages_xml(package, xml, None)
}

/// Turn shown page `index` into a background page (it moves to the front of pages.xml like in Visio, and no longer shows as a
/// tab). Refused for the last foreground page and for a page that has a background of its own.
pub fn make_background(package: &Package, index: usize) -> Result<Vec<u8>> {
    let pages = Pages::load(package)?;
    let page = pages.get(index)?.clone();
    if pages.shown.len() == 1 {
        return Err(Error("a drawing needs at least one page that is not a background".to_string()));
    }
    if page.back.is_some() {
        return Err(Error(format!("page {} has a background of its own: clear it first", page.name)));
    }
    let (a, b) = open_tag_range(&pages.xml, page.start);
    let open = set_attr(&pages.xml[a..b], "Background", "1");
    // The page element goes to the front of the list; the text around the elements stays.
    let slot = pages.slot(index)?;
    let mut elements: Vec<String> = pages.list.iter().map(|p| pages.xml[p.start..p.end].to_string()).collect();
    let mut moved = elements.remove(slot);
    let open_end = moved.find('>').map_or(0, |n| n + 1);
    moved.replace_range(..open_end, &open);
    elements.insert(0, moved);
    let mut xml = String::new();
    let mut cursor = 0;
    for (place, element) in pages.list.iter().zip(&elements) {
        xml.push_str(&pages.xml[cursor..place.start]);
        xml.push_str(element);
        cursor = place.end;
    }
    xml.push_str(&pages.xml[cursor..]);
    // The window must not stay on a page that is no longer shown.
    let mut windows_edit = None;
    if package.contains("visio/windows.xml") {
        let windows = package.text("visio/windows.xml")?;
        if attr_in(windows, "Page").as_deref() == Some(page.id.as_str()) {
            let fallback = pages.shown.iter().map(|&i| &pages.list[i]).find(|p| p.id != page.id).map(|p| p.id.clone()).unwrap_or_else(|| "0".to_string());
            windows_edit = Some(("visio/windows.xml".to_string(), windows.replace(&format!("Page='{}'", page.id), &format!("Page='{fallback}'")).replace(&format!("Page=\"{}\"", page.id), &format!("Page=\"{fallback}\"")).into_bytes()));
        }
    }
    edit_pages_xml(package, xml, windows_edit)
}

/// Turn background page `id` back into an ordinary page; pages that showed it lose that background. Returns the package bytes and
/// the shell index of the page that is now shown.
pub fn make_foreground(package: &Package, id: &str) -> Result<(Vec<u8>, usize)> {
    let pages = Pages::load(package)?;
    let page = pages.list.iter().find(|p| p.id == id && p.background).ok_or_else(|| Error(format!("page {id} is not a background page")))?.clone();
    let mut xml = pages.xml.clone();
    // Back to front so earlier ranges stay valid: drop BackPage from every page that used it, then the page's own flag.
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for p in &pages.list {
        let (a, b) = open_tag_range(&pages.xml, p.start);
        let mut open = pages.xml[a..b].to_string();
        if p.back.as_deref() == Some(id) {
            open = remove_attr(&open, "BackPage");
        }
        if p.id == page.id {
            open = remove_attr(&open, "Background");
        }
        if open != pages.xml[a..b] {
            edits.push((a, b, open));
        }
    }
    for (a, b, open) in edits.into_iter().rev() {
        xml.replace_range(a..b, &open);
    }
    let bytes = edit_pages_xml(package, xml, None)?;
    let shown_index = {
        let again = Pages::load(&Package::from_bytes(&bytes)?)?;
        again.shown.iter().position(|&i| again.list[i].id == id).unwrap_or(0)
    };
    Ok((bytes, shown_index))
}

// ---- drawing scale -----------------------------------------------------------------------------------------------------
// Visio (probe-drawing-scale.ps1): the page sheet keeps the scale as `PageScale` (paper) and `DrawingScale` (the real world), both
// as VALUES IN INCHES with the display unit in `U` (1/4 in = 1 ft is PageScale V='0.25' U='IN', DrawingScale V='12' U='FT'; 1:50
// metric is V='0.0393..' U='MM' and V='1.9685..' U='MM'), plus `DrawingScaleType` (0 no scale, 1 architectural, 4 metric; 5 custom is
// assumed). The shapes' own cells do not change and the paper size (PageWidth / PageHeight) stays; only what the shapes mean in
// the real world does. A page with no scale is 1:1.

/// Inches per unit of a page-sheet unit code (`None` = not a length unit this editor offers).
pub fn unit_inches(code: &str) -> Option<f64> {
    Some(match code.to_ascii_uppercase().as_str() {
        "IN" | "IN_F" => 1.0,
        "FT" | "F_I" => 12.0,
        "MM" => 1.0 / 25.4,
        "CM" => 1.0 / 2.54,
        "M" => 1000.0 / 25.4,
        _ => return None,
    })
}

/// A page's scale as the page sheet states it: the amounts in their display units.
#[derive(Debug, Clone, PartialEq)]
pub struct PageScaleInfo {
    pub paper: f64,
    pub paper_unit: String,
    pub drawing: f64,
    pub drawing_unit: String,
    pub scale_type: i32,
}

fn sheet_cell_tag(entry: &str, name: &str) -> Option<String> {
    let at = entry.find(&format!("<Cell N='{name}'"))?;
    let end = entry[at..].find('>').map_or(entry.len(), |n| at + n + 1);
    Some(entry[at..end].to_string())
}

fn amount_and_unit(entry: &str, name: &str) -> (f64, String) {
    match sheet_cell_tag(entry, name) {
        Some(tag) => {
            let unit = attr(&tag, "U").unwrap_or_else(|| "IN".to_string());
            let inches = attr(&tag, "V").and_then(|v| v.parse::<f64>().ok()).unwrap_or(1.0);
            (inches / unit_inches(&unit).unwrap_or(1.0), unit)
        }
        None => (1.0, "IN".to_string()),
    }
}

/// `PageScale / DrawingScale` of shown page `index` (1 for an unscaled page, 0.1 for 1 in = 10 in, 2 for 2 in = 1 in).
pub fn page_scale_ratio(package: &Package, index: usize) -> Result<f64> {
    let info = page_scale(package, index)?;
    let paper = info.paper * unit_inches(&info.paper_unit).unwrap_or(1.0);
    let drawing = info.drawing * unit_inches(&info.drawing_unit).unwrap_or(1.0);
    Ok(if paper > 0.0 && drawing > 0.0 { paper / drawing } else { 1.0 })
}

/// The scale of shown page `index`.
pub fn page_scale(package: &Package, index: usize) -> Result<PageScaleInfo> {
    let pages = Pages::load(package)?;
    let page = pages.get(index)?;
    let entry = &pages.xml[page.start..page.end];
    let sheet = entry.find("<PageSheet").map_or("", |s| &entry[s..entry[s..].find("</PageSheet>").map_or(entry.len(), |e| s + e)]);
    let (paper, paper_unit) = amount_and_unit(sheet, "PageScale");
    let (drawing, drawing_unit) = amount_and_unit(sheet, "DrawingScale");
    let scale_type = sheet_cell_tag(sheet, "DrawingScaleType").and_then(|t| attr(&t, "V")).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0) as i32;
    Ok(PageScaleInfo { paper, paper_unit, drawing, drawing_unit, scale_type })
}

/// The avenue size of shown page `index` (the cells AvenueSizeX / AvenueSizeY of the page sheet, inches): the clear space Visio leaves between a shape and the one it places next
/// to it (AutoConnect, Auto Space; 0.295 in = 7.5 mm on Visio's A4 default page, probe-autoconnect.ps1). None when the page sheet has no such cells.
pub fn avenue_size(package: &Package, index: usize) -> Result<Option<(f64, f64)>> {
    let pages = Pages::load(package)?;
    let page = pages.get(index)?;
    let entry = &pages.xml[page.start..page.end];
    let sheet = entry.find("<PageSheet").map_or("", |s| &entry[s..entry[s..].find("</PageSheet>").map_or(entry.len(), |e| s + e)]);
    let get = |name: &str| sheet_cell_tag(sheet, name).and_then(|t| attr(&t, "V")).and_then(|v| v.parse::<f64>().ok());
    Ok(match (get("AvenueSizeX"), get("AvenueSizeY")) {
        (Some(x), Some(y)) => Some((x, y)),
        _ => None,
    })
}

/// Whether the unit code is a metric one (a page whose paper unit is metric is shown in millimetres).
pub fn is_metric_unit(code: &str) -> bool {
    matches!(code.to_ascii_uppercase().as_str(), "MM" | "CM" | "M" | "KM")
}

/// Replace cell `name` of a page sheet (or add it right after the sheet's start tag) with `V` and optional `U`.
fn put_sheet_cell(entry: &mut String, name: &str, value: &str, unit: Option<&str>) -> Result<()> {
    let sheet_open = entry.find("<PageSheet").ok_or_else(|| Error("the page has no page sheet".to_string()))?;
    let sheet_end = entry[sheet_open..].find('>').map(|n| sheet_open + n + 1).ok_or_else(|| Error("bad page sheet".to_string()))?;
    let cell = match unit {
        Some(u) => format!("<Cell N='{name}' V='{value}' U='{u}'/>"),
        None => format!("<Cell N='{name}' V='{value}'/>"),
    };
    let key = format!("<Cell N='{name}'");
    match entry[sheet_open..].find(&key).map(|n| n + sheet_open) {
        Some(at) => {
            let close = entry[at..].find('>').map_or(entry.len(), |n| at + n + 1);
            let self_closed = entry[at..close].ends_with("/>");
            let end = if self_closed { close } else { entry[close..].find("</Cell>").map_or(close, |n| close + n + "</Cell>".len()) };
            entry.replace_range(at..end, &cell);
        }
        None => entry.insert_str(sheet_end, &cell),
    }
    Ok(())
}

/// Set the drawing scale of shown page `index` to `paper` `paper_unit` = `drawing` `drawing_unit` (units `in ft mm cm m`; no scale is
/// 1 = 1). `DrawingScaleType` follows: 0 for 1:1, 1 for inches to feet, 4 when both are metric, else 5.
pub fn set_drawing_scale(package: &Package, index: usize, paper: f64, paper_unit: &str, drawing: f64, drawing_unit: &str) -> Result<Vec<u8>> {
    for (name, value) in [("paper", paper), ("drawing", drawing)] {
        if !value.is_finite() || value <= 0.0 || value > 1e9 {
            return Err(Error(format!("the {name} length of the scale must be a positive number")));
        }
    }
    let unit = |code: &str| -> Result<(String, f64)> {
        let up = code.trim().to_ascii_uppercase();
        let factor = unit_inches(&up).filter(|_| up != "IN_F" && up != "F_I").ok_or_else(|| Error(format!("unknown unit {code}: use in, ft, mm, cm or m")))?;
        Ok((up, factor))
    };
    let ((pu, pf), (du, df)) = (unit(paper_unit)?, unit(drawing_unit)?);
    let ratio = (paper * pf) / (drawing * df);
    if !(1e-4..=1e4).contains(&ratio) {
        return Err(Error("the scale must be between 1:10000 and 10000:1".to_string()));
    }
    let scale_type = if (ratio - 1.0).abs() < 1e-9 {
        0
    } else if pu == "IN" && du == "FT" {
        1
    } else if is_metric_unit(&pu) && is_metric_unit(&du) {
        4
    } else {
        5
    };
    let pages = Pages::load(package)?;
    let page = pages.get(index)?.clone();
    let mut xml = pages.xml.clone();
    let mut entry = xml[page.start..page.end].to_string();
    let num = |v: f64| format!("{}", (v * 1e12).round() / 1e12);
    put_sheet_cell(&mut entry, "PageScale", &num(paper * pf), Some(&pu))?;
    put_sheet_cell(&mut entry, "DrawingScale", &num(drawing * df), Some(&du))?;
    put_sheet_cell(&mut entry, "DrawingScaleType", &scale_type.to_string(), None)?;
    xml.replace_range(page.start..page.end, &entry);
    edit_pages_xml(package, xml, None)
}

/// A page's print setup (Visio's Page Setup > Print Setup tab), stored in cells of the page sheet. Visio writes only the cells that differ from
/// their defaults (probed with visio-test-harness/probe-print-setup.ps1): margins 0.25 in, scale 1, 1 x 1 sheets, not centred, no fit.
#[derive(Debug, Clone, PartialEq)]
pub struct PrintSetup {
    /// `PrintPageOrientation`: 1 portrait, 2 landscape.
    pub orientation: u32,
    /// `PageLeftMargin`, `PageRightMargin`, `PageTopMargin`, `PageBottomMargin` (inches).
    pub margins: [f64; 4],
    /// `PrintGrid`: print the grid lines.
    pub grid: bool,
    /// `ScaleX`, `ScaleY`: the print zoom factor (1 = 100 %), used when `fit` is off.
    pub scale: (f64, f64),
    /// `OnPage` = 1: fit the drawing on `PagesX` x `PagesY` sheets instead of using the scale.
    pub fit: bool,
    pub pages: (u32, u32),
    /// `CenterX`, `CenterY`: centre the drawing on the sheet.
    pub center: (bool, bool),
}

impl Default for PrintSetup {
    fn default() -> Self {
        PrintSetup { orientation: 1, margins: [0.25; 4], grid: false, scale: (1.0, 1.0), fit: false, pages: (1, 1), center: (false, false) }
    }
}

fn sheet_value(sheet: &str, name: &str) -> Option<f64> {
    sheet_cell_tag(sheet, name).and_then(|t| attr(&t, "V")).and_then(|v| v.parse::<f64>().ok())
}

/// The print setup of shown page `index` (cells the sheet does not state have their default).
pub fn print_setup(package: &Package, index: usize) -> Result<PrintSetup> {
    let pages = Pages::load(package)?;
    let page = pages.get(index)?;
    let entry = &pages.xml[page.start..page.end];
    let sheet = entry.find("<PageSheet").map_or("", |s| &entry[s..entry[s..].find("</PageSheet>").map_or(entry.len(), |e| s + e)]);
    let d = PrintSetup::default();
    let get = |name: &str, default: f64| sheet_value(sheet, name).unwrap_or(default);
    Ok(PrintSetup {
        orientation: if get("PrintPageOrientation", 1.0) as u32 == 2 { 2 } else { 1 },
        margins: [get("PageLeftMargin", d.margins[0]), get("PageRightMargin", d.margins[1]), get("PageTopMargin", d.margins[2]), get("PageBottomMargin", d.margins[3])],
        grid: get("PrintGrid", 0.0) != 0.0,
        scale: (get("ScaleX", 1.0), get("ScaleY", 1.0)),
        fit: get("OnPage", 0.0) != 0.0,
        pages: (get("PagesX", 1.0).max(1.0) as u32, get("PagesY", 1.0).max(1.0) as u32),
        center: (get("CenterX", 0.0) != 0.0, get("CenterY", 0.0) != 0.0),
    })
}

/// The printer paper of shown page `index` in inches (width, height), turned to the page's print orientation (landscape: the long side is the width): the
/// `PaperKind` cell (1 Letter, 3 Tabloid, 5 Legal, 7 Executive, 8 A3, 9 A4, 11 A5, 13 B5) with `PrintPageOrientation`. `None` for a kind not in the
/// table. Measured with probe-print-sizes.ps1 (step 255): the printed sheets have exactly this size.
pub fn paper_size(package: &Package, index: usize) -> Result<Option<(f64, f64)>> {
    let pages = Pages::load(package)?;
    let page = pages.get(index)?;
    let entry = &pages.xml[page.start..page.end];
    let sheet = entry.find("<PageSheet").map_or("", |s| &entry[s..entry[s..].find("</PageSheet>").map_or(entry.len(), |e| s + e)]);
    // a file that does not state PaperKind leaves the paper to the printer: nothing is known then, the drawing page stands for it
    let Some(kind) = sheet_value(sheet, "PaperKind").map(|k| k as u32) else { return Ok(None) };
    let (short, long) = match kind {
        1 => (8.5, 11.0),
        3 => (11.0, 17.0),
        5 => (8.5, 14.0),
        7 => (7.25, 10.5),
        8 => (11.693, 16.535),
        9 => (8.268, 11.693),
        11 => (5.827, 8.268),
        13 => (7.165, 10.118),
        _ => return Ok(None),
    };
    Ok(Some(if sheet_value(sheet, "PrintPageOrientation").unwrap_or(1.0) as u32 == 2 { (long, short) } else { (short, long) }))
}

/// Apply gallery theme `name` (see `themes::all`) to the drawing the way Visio's Design > Themes does (probe-theme-apply.ps1, step 257): every theme
/// part of the package is replaced by the theme's DrawingML part (the importer reads the document's theme from there; shapes keep their `THEMEVAL()`
/// formulas) and the page sheets get the five scheme cells with the gallery index. A drawing without a theme part gets one (part, content type, relationship).
pub fn apply_theme(package: &Package, name: &str) -> Result<Vec<u8>> {
    apply_theme_page(package, name, None)
}

/// The page sheet cells of every page: `(ThemeIndex, VariationColorIndex)`, None where `ThemeIndex` is not stated.
pub fn page_themes(package: &Package) -> Vec<(Option<u32>, u32)> {
    let Ok(pages) = Pages::load(package) else { return Vec::new() };
    let mut out = Vec::new();
    let mut i = 0;
    while let Ok(page) = pages.get(i) {
        let entry = &pages.xml[page.start..page.end];
        let sheet = entry.find("<PageSheet").map_or("", |s| &entry[s..entry[s..].find("</PageSheet>").map_or(entry.len(), |e| s + e)]);
        out.push((sheet_value(sheet, "ThemeIndex").map(|v| v.max(0.0) as u32), (sheet_value(sheet, "VariationColorIndex").unwrap_or(0.0).max(0.0) as u32).min(3)));
        i += 1;
    }
    out
}

/// Apply theme `name` to page `page` only (Visio: `Page.SetTheme`, probe-page-themes.ps1, step 272), or to every page when `page` is None. A theme per page is a theme PART per
/// theme (theme1.xml, theme2.xml, ... all related from document.xml.rels); a page follows the part whose `schemeEnum` equals its `ThemeIndex`, the part order does not matter, and
/// a page that never got a theme has `ThemeIndex` 0 and looks as if the drawing had none.
pub fn apply_theme_page(package: &Package, name: &str, page: Option<usize>) -> Result<Vec<u8>> {
    let (theme_name, index) = crate::themes::own_name(name).and_then(|own| crate::themes::all().into_iter().find(|(n, _)| *n == own)).ok_or_else(|| Error(format!("no theme named {name}")))?;
    let parts: Vec<String> = package.names().filter(|n| { let l = n.to_ascii_lowercase(); l.starts_with("visio/theme/theme") && l.ends_with(".xml") }).map(str::to_string).collect();
    let mut extras: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let enum_of = |part: &str| -> Option<u32> {
        let text = package.text(part).ok()?;
        let at = text.find("schemeEnum=")? + "schemeEnum=".len() + 1;
        text[at..].split(['"', '\'']).next()?.parse().ok()
    };
    // one page: reuse the part of this theme when there is one, otherwise add the next theme part beside the others
    let mut new_part: Option<String> = None;
    let parts = if page.is_some() && !parts.is_empty() {
        if !parts.iter().any(|p| enum_of(p) == Some(index)) {
            let max = parts.iter().filter_map(|p| p.trim_start_matches("visio/theme/theme").trim_end_matches(".xml").parse::<u32>().ok()).max().unwrap_or(0);
            let part = format!("visio/theme/theme{}.xml", max + 1);
            let types = package.text("[Content_Types].xml")?.to_string();
            let rels_part = "visio/_rels/document.xml.rels";
            let rels = package.text(rels_part)?.to_string();
            let override_tag = format!("<Override PartName=\"/{part}\" ContentType=\"application/vnd.openxmlformats-officedocument.theme+xml\"/>");
            let mut max_id = 0u64;
            for chunk in rels.split("Id=").skip(1) {
                let digits: String = chunk.trim_start_matches(['"', '\'']).strip_prefix("rId").unwrap_or("").chars().take_while(|c| c.is_ascii_digit()).collect();
                max_id = max_id.max(digits.parse::<u64>().unwrap_or(0));
            }
            let rel = format!("<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme\" Target=\"theme/theme{}.xml\"/>", max_id + 1, max + 1);
            if !rels.contains("</Relationships>") {
                return Err(Error("the document part has no relationships".to_string()));
            }
            extras.insert("[Content_Types].xml".to_string(), types.replacen("</Types>", &format!("{override_tag}</Types>"), 1).into_bytes());
            extras.insert(rels_part.to_string(), rels.replacen("</Relationships>", &format!("{rel}</Relationships>"), 1).into_bytes());
            new_part = Some(part);
        }
        parts
    } else if parts.is_empty() {
        // a drawing without a theme part (the writer of some tools leaves it out): add `visio/theme/theme1.xml` with its content type and its relationship
        // from the document part, as Visio itself writes them (probe-theme-apply.ps1)
        let types = package.text("[Content_Types].xml")?.to_string();
        let rels_part = "visio/_rels/document.xml.rels";
        let rels = package.text(rels_part)?.to_string();
        let override_tag = "<Override PartName=\"/visio/theme/theme1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.theme+xml\"/>";
        let types = types.replacen("</Types>", &format!("{override_tag}</Types>"), 1);
        let mut max = 0u64;
        for chunk in rels.split("Id=").skip(1) {
            let digits: String = chunk.trim_start_matches(['"', '\'']).strip_prefix("rId").unwrap_or("").chars().take_while(|c| c.is_ascii_digit()).collect();
            max = max.max(digits.parse::<u64>().unwrap_or(0));
        }
        let rel = format!("<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme\" Target=\"theme/theme1.xml\"/>", max + 1);
        if !rels.contains("</Relationships>") {
            return Err(Error("the document part has no relationships".to_string()));
        }
        extras.insert("[Content_Types].xml".to_string(), types.into_bytes());
        extras.insert(rels_part.to_string(), rels.replacen("</Relationships>", &format!("{rel}</Relationships>"), 1).into_bytes());
        new_part = Some("visio/theme/theme1.xml".to_string());
        vec!["visio/theme/theme1.xml".to_string()]
    } else {
        parts
    };
    let pages = Pages::load(package)?;
    let mut xml_pages = pages.xml.clone();
    let mut ranges = Vec::new();
    let mut i = 0;
    while let Ok(page) = pages.get(i) {
        ranges.push((page.start, page.end));
        i += 1;
    }
    if let Some(p) = page {
        if p >= ranges.len() {
            return Err(Error(format!("the drawing has no page {p}")));
        }
    }
    // the theme the other pages follow today (the one part of a drawing with a single theme): pages without a ThemeIndex keep it, now stated explicitly
    // (only for a drawing in which no page states a theme yet: once a page does, a page without the cell is a page without a theme)
    let legacy = page_themes(package).iter().all(|t| t.0.unwrap_or(0) == 0);
    let old_theme = if page.is_some() && parts.len() == 1 && legacy { enum_of(&parts[0]) } else { None };
    for (n, (start, end)) in ranges.into_iter().enumerate().rev() {
        let mut entry = xml_pages[start..end].to_string();
        if page.map_or(true, |p| p == n) {
            for cell in ["ColorSchemeIndex", "EffectSchemeIndex", "ConnectorSchemeIndex", "FontSchemeIndex", "ThemeIndex"] {
                put_sheet_cell(&mut entry, cell, &index.to_string(), None)?;
            }
            // a new theme starts at its first variant (Visio: Page.SetTheme leaves both variation cells 0, probe-variants.ps1)
            for cell in ["VariationColorIndex", "VariationStyleIndex"] {
                put_sheet_cell(&mut entry, cell, "0", None)?;
            }
        } else if let Some(old) = old_theme {
            let sheet = entry.find("<PageSheet").map_or("", |s| &entry[s..entry[s..].find("</PageSheet>").map_or(entry.len(), |e| s + e)]);
            if sheet_value(sheet, "ThemeIndex").is_none() {
                put_sheet_cell(&mut entry, "ThemeIndex", &old.to_string(), None)?;
            }
        }
        xml_pages.replace_range(start..end, &entry);
    }
    extras.insert(PAGES.to_string(), xml_pages.into_bytes());
    // one page: theme parts that no page follows any more are dropped (part, relationship, content type), as Visio does not keep them
    let mut removed: Vec<String> = Vec::new();
    if let Some(p) = page {
        let mut used: Vec<u32> = page_themes(package).into_iter().enumerate().map(|(n, t)| if n == p { Some(index) } else { t.0.or(old_theme) }).flatten().filter(|i| *i != 0).collect();
        used.sort_unstable();
        let unused: Vec<String> = parts.iter().filter(|part| enum_of(part).is_some_and(|e| !used.contains(&e))).cloned().collect();
        if !unused.is_empty() && unused.len() < parts.len() + usize::from(new_part.is_some()) {
            let rels_part = "visio/_rels/document.xml.rels";
            let mut rels = match extras.get(rels_part) { Some(b) => String::from_utf8_lossy(b).to_string(), None => package.text(rels_part)?.to_string() };
            let mut types = match extras.get("[Content_Types].xml") { Some(b) => String::from_utf8_lossy(b).to_string(), None => package.text("[Content_Types].xml")?.to_string() };
            for part in &unused {
                let file = part.trim_start_matches("visio/");
                if let Some(at) = rels.find(&format!("Target=\"{file}\"")).or_else(|| rels.find(&format!("Target='{file}'"))) {
                    let start = rels[..at].rfind("<Relationship ").unwrap_or(0);
                    let end = rels[at..].find("/>").map_or(rels.len(), |n| at + n + 2);
                    rels.replace_range(start..end, "");
                }
                for key in [format!("<Override PartName=\"/{part}\""), format!("<Override PartName='/{part}'")] {
                    if let Some(at) = types.find(&key) {
                        let end = types[at..].find("/>").map_or(types.len(), |n| at + n + 2);
                        types.replace_range(at..end, "");
                    }
                }
                removed.push(part.clone());
            }
            extras.insert(rels_part.to_string(), rels.into_bytes());
            extras.insert("[Content_Types].xml".to_string(), types.into_bytes());
        }
    }
    match (&new_part, page) {
        (Some(part), _) => {
            extras.insert(part.clone(), crate::themes::part(theme_name).into_bytes());
        }
        (None, Some(_)) => {} // the part of this theme exists already
        (None, None) => {
            for part in parts {
                extras.insert(part, crate::themes::part(theme_name).into_bytes());
            }
        }
    }
    write_package_edit(package, extras, &removed)
}

/// Choose variant `variant` (0 to 3) of the theme the drawing has (Design > Variants). Measured with probe-variants.ps1 (round 12): Visio keeps the variant in the two page sheet
/// cells `VariationColorIndex` and `VariationStyleIndex` (both set to the variant number; 0 = the theme's own look) and picks the colours and style indices from the theme part's
/// `variationClrSchemeLst` / `variationStyleSchemeLst`; for Integral variant 1 turns a rectangle's fill from RGB(0; 131; 185) into RGB(156; 190; 189).
pub fn set_theme_variant(package: &Package, variant: u32) -> Result<Vec<u8>> {
    set_theme_variant_page(package, variant, None)
}

/// The same for one page (`page` = Some) or every page (None); a page without a theme (`ThemeIndex` 0) has no variants.
pub fn set_theme_variant_page(package: &Package, variant: u32, page: Option<usize>) -> Result<Vec<u8>> {
    if variant > 3 {
        return Err(Error("a theme has the variants 0 to 3".to_string()));
    }
    if !package.names().any(|n| { let l = n.to_ascii_lowercase(); l.starts_with("visio/theme/theme") && l.ends_with(".xml") }) {
        return Err(Error("the drawing has no theme: apply one first".to_string()));
    }
    let pages = Pages::load(package)?;
    let mut xml_pages = pages.xml.clone();
    let mut ranges = Vec::new();
    let mut i = 0;
    while let Ok(page) = pages.get(i) {
        ranges.push((page.start, page.end));
        i += 1;
    }
    if let Some(p) = page {
        if p >= ranges.len() {
            return Err(Error(format!("the drawing has no page {p}")));
        }
        if page_themes(package).get(p).and_then(|t| t.0) == Some(0) {
            return Err(Error("this page has no theme: apply one first".to_string()));
        }
    }
    for (n, (start, end)) in ranges.into_iter().enumerate().rev() {
        if page.is_some_and(|p| p != n) {
            continue;
        }
        let mut entry = xml_pages[start..end].to_string();
        for cell in ["VariationColorIndex", "VariationStyleIndex"] {
            put_sheet_cell(&mut entry, cell, &variant.to_string(), None)?;
        }
        xml_pages.replace_range(start..end, &entry);
    }
    edit_pages_xml(package, xml_pages, None)
}

/// The variant (0 to 3) of the first page's theme (`VariationColorIndex`; 0 when the cell is not stated).
pub fn theme_variant(package: &Package) -> u32 {
    let Ok(pages) = Pages::load(package) else { return 0 };
    let Ok(page) = pages.get(0) else { return 0 };
    let entry = &pages.xml[page.start..page.end];
    let sheet = entry.find("<PageSheet").map_or("", |s| &entry[s..entry[s..].find("</PageSheet>").map_or(entry.len(), |e| s + e)]);
    (sheet_value(sheet, "VariationColorIndex").unwrap_or(0.0).max(0.0) as u32).min(3)
}

/// Remove cell `name` from a page sheet (nothing happens when it is not there).
fn remove_sheet_cell(entry: &mut String, name: &str) {
    let Some(open) = entry.find("<PageSheet") else { return };
    let key = format!("<Cell N='{name}'");
    if let Some(at) = entry[open..].find(&key).map(|n| n + open) {
        let close = entry[at..].find('>').map_or(entry.len(), |n| at + n + 1);
        let end = if entry[at..close].ends_with("/>") { close } else { entry[close..].find("</Cell>").map_or(close, |n| close + n + "</Cell>".len()) };
        entry.replace_range(at..end, "");
    }
}

/// Write the print setup of shown page `index`: the cells that differ from their default are written, the others removed, the way Visio keeps them.
pub fn set_print_setup(package: &Package, index: usize, setup: &PrintSetup) -> Result<Vec<u8>> {
    let d = PrintSetup::default();
    for v in setup.margins {
        if !v.is_finite() || !(0.0..=20.0).contains(&v) {
            return Err(Error("a print margin must be between 0 and 20 in".to_string()));
        }
    }
    for v in [setup.scale.0, setup.scale.1] {
        if !v.is_finite() || !(0.01..=100.0).contains(&v) {
            return Err(Error("the print zoom must be between 1 % and 10000 %".to_string()));
        }
    }
    if !(1..=99).contains(&setup.pages.0) || !(1..=99).contains(&setup.pages.1) {
        return Err(Error("fit on 1 to 99 sheets in each direction".to_string()));
    }
    let pages = Pages::load(package)?;
    let page = pages.get(index)?.clone();
    let mut xml = pages.xml.clone();
    let mut entry = xml[page.start..page.end].to_string();
    let num = |v: f64| format!("{}", (v * 1e12).round() / 1e12);
    let put = |entry: &mut String, name: &str, value: f64, default: f64| -> Result<()> {
        if (value - default).abs() < 1e-12 {
            remove_sheet_cell(entry, name);
            Ok(())
        } else {
            put_sheet_cell(entry, name, &num(value), None)
        }
    };
    // PrintPageOrientation is always written (Visio's own files carry it)
    put_sheet_cell(&mut entry, "PrintPageOrientation", &setup.orientation.to_string(), None)?;
    put(&mut entry, "PageLeftMargin", setup.margins[0], d.margins[0])?;
    put(&mut entry, "PageRightMargin", setup.margins[1], d.margins[1])?;
    put(&mut entry, "PageTopMargin", setup.margins[2], d.margins[2])?;
    put(&mut entry, "PageBottomMargin", setup.margins[3], d.margins[3])?;
    put(&mut entry, "PrintGrid", if setup.grid { 1.0 } else { 0.0 }, 0.0)?;
    put(&mut entry, "ScaleX", setup.scale.0, 1.0)?;
    put(&mut entry, "ScaleY", setup.scale.1, 1.0)?;
    put(&mut entry, "OnPage", if setup.fit { 1.0 } else { 0.0 }, 0.0)?;
    put(&mut entry, "PagesX", f64::from(setup.pages.0), 1.0)?;
    put(&mut entry, "PagesY", f64::from(setup.pages.1), 1.0)?;
    put(&mut entry, "CenterX", if setup.center.0 { 1.0 } else { 0.0 }, 0.0)?;
    put(&mut entry, "CenterY", if setup.center.1 { 1.0 } else { 0.0 }, 0.0)?;
    xml.replace_range(page.start..page.end, &entry);
    edit_pages_xml(package, xml, None)
}
