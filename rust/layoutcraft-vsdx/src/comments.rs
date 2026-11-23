//! Comments (Visio's "Review > New Comment"): the part `visio/comments.xml`.
//!
//! Visio (probe-comments.ps1, Visio 16.0, test/corpus/comments.vsdx): comments live in ONE part, `/visio/comments.xml`
//! (content type `application/vnd.ms-visio.comments+xml`), related from `visio/_rels/document.xml.rels` with the type
//! `http://schemas.microsoft.com/visio/2010/relationships/comments`; the pages and shapes carry nothing. The root is
//! `<Comments ShowCommentTags='1' xmlns='http://schemas.microsoft.com/office/visio/2012/main' ...>` with an `AuthorList` of
//! `AuthorEntry Name Initials ResolutionID ID` and a `CommentList` of `CommentEntry AuthorID PageID [ShapeID] Date EditDate CommentID
//! AutoCommentType` whose text is the comment (newlines kept, `xml:space='preserve'`). `PageID` is the page's `ID` in pages.xml, a comment
//! on the page itself has no `ShapeID`. Dates are local time `YYYY-MM-DDTHH:MM:SS`. COM reads them as `Shape.Comments`, `Page.Comments`
//! (all comments of the page), `Document.Comments` with Text, AuthorName, AuthorInitials, CreateDate, EditDate, AssociatedObject.

use std::collections::BTreeMap;

use crate::writer::{escape_attr, find_shape_range, write_package_edit};
use crate::{Error, Package, Result};

const PART: &str = "visio/comments.xml";
const DOC_RELS: &str = "visio/_rels/document.xml.rels";
const TYPES: &str = "[Content_Types].xml";
const REL_TYPE: &str = "http://schemas.microsoft.com/visio/2010/relationships/comments";
const CONTENT_TYPE: &str = "application/vnd.ms-visio.comments+xml";
const ROOT_OPEN: &str = "<Comments ShowCommentTags='1' xmlns='http://schemas.microsoft.com/office/visio/2012/main' xmlns:r='http://schemas.openxmlformats.org/officeDocument/2006/relationships' xml:space='preserve'>";
const MAX_COMMENTS: usize = 2000;
const MAX_TEXT: usize = 32_000;
const MAX_NAME: usize = 255;

/// A comment as the shell shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct CommentInfo {
    pub id: String,
    pub author: String,
    pub initials: String,
    /// The index of the page the comment belongs to (None when its `PageID` matches no page).
    pub page_index: Option<usize>,
    /// The shape it is attached to; None for a comment on the page itself.
    pub shape_id: Option<String>,
    pub date: String,
    pub edit_date: String,
    pub text: String,
}

#[derive(Debug, Clone)]
struct Author {
    id: String,
    attrs: Vec<(String, String)>,
}
impl Author {
    fn get(&self, name: &str) -> &str {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str()).unwrap_or("")
    }
}
#[derive(Debug, Clone)]
struct Entry {
    attrs: Vec<(String, String)>,
    text: String,
}
impl Entry {
    fn get(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
    fn set(&mut self, name: &str, value: &str) {
        match self.attrs.iter_mut().find(|(k, _)| k == name) {
            Some(slot) => slot.1 = value.to_string(),
            None => self.attrs.push((name.to_string(), value.to_string())),
        }
    }
}
struct Parsed {
    root_open: String,
    authors: Vec<Author>,
    entries: Vec<Entry>,
}

fn parse(xml: &str) -> Result<Parsed> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| Error(format!("comments.xml: {e}")))?;
    let root = doc.root_element();
    if root.tag_name().name() != "Comments" {
        return Err(Error("comments.xml has no Comments element".to_string()));
    }
    let start = xml.find("<Comments").ok_or_else(|| Error("comments.xml has no Comments element".to_string()))?;
    let end = xml[start..].find('>').ok_or_else(|| Error("comments.xml is cut off".to_string()))? + start + 1;
    let attrs_of = |n: roxmltree::Node| -> Vec<(String, String)> { n.attributes().map(|a| (a.name().to_string(), a.value().to_string())).collect() };
    let mut authors = Vec::new();
    let mut entries = Vec::new();
    for list in root.children().filter(|n| n.is_element()) {
        match list.tag_name().name() {
            "AuthorList" => {
                for a in list.children().filter(|n| n.is_element() && n.tag_name().name() == "AuthorEntry") {
                    authors.push(Author { id: a.attribute("ID").unwrap_or("").to_string(), attrs: attrs_of(a) });
                }
            }
            "CommentList" => {
                for c in list.children().filter(|n| n.is_element() && n.tag_name().name() == "CommentEntry") {
                    entries.push(Entry { attrs: attrs_of(c), text: c.text().unwrap_or("").to_string() });
                }
            }
            _ => {}
        }
    }
    Ok(Parsed { root_open: xml[start..end].to_string(), authors, entries })
}

fn escape_text(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn serialize(p: &Parsed) -> String {
    let mut out = String::from("<?xml version='1.0' encoding='utf-8' ?>\n");
    out.push_str(&p.root_open);
    out.push_str("<AuthorList>");
    for a in &p.authors {
        out.push_str("<AuthorEntry");
        for (k, v) in &a.attrs {
            out.push_str(&format!(" {k}='{}'", escape_attr(v)));
        }
        out.push_str("/>");
    }
    out.push_str("</AuthorList><CommentList>");
    for e in &p.entries {
        out.push_str("<CommentEntry");
        for (k, v) in &e.attrs {
            out.push_str(&format!(" {k}='{}'", escape_attr(v)));
        }
        out.push('>');
        out.push_str(&escape_text(&e.text));
        out.push_str("</CommentEntry>");
    }
    out.push_str("</CommentList></Comments>");
    out
}

fn load(package: &Package) -> Result<Option<Parsed>> {
    match package.text(PART) {
        Ok(xml) => parse(xml).map(Some),
        Err(_) => Ok(None),
    }
}

/// All comments of the drawing in file order.
pub fn list_comments(package: &Package) -> Result<Vec<CommentInfo>> {
    let Some(parsed) = load(package)? else { return Ok(Vec::new()) };
    let pages = package.pages()?;
    let mut out = Vec::new();
    for e in &parsed.entries {
        let author = parsed.authors.iter().find(|a| Some(a.id.as_str()) == e.get("AuthorID"));
        let page_id = e.get("PageID").unwrap_or("");
        out.push(CommentInfo {
            id: e.get("CommentID").unwrap_or("").to_string(),
            author: author.map(|a| a.get("Name").to_string()).unwrap_or_default(),
            initials: author.map(|a| a.get("Initials").to_string()).unwrap_or_default(),
            page_index: pages.iter().position(|p| p.id.as_deref() == Some(page_id)),
            shape_id: e.get("ShapeID").map(str::to_string),
            date: e.get("Date").unwrap_or("").to_string(),
            edit_date: e.get("EditDate").unwrap_or("").to_string(),
            text: e.text.clone(),
        });
    }
    Ok(out)
}

fn check_text(text: &str) -> Result<()> {
    if text.trim().is_empty() {
        return Err(Error("a comment needs some text".to_string()));
    }
    if text.chars().count() > MAX_TEXT {
        return Err(Error(format!("a comment is limited to {MAX_TEXT} characters")));
    }
    if text.chars().any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t')) {
        return Err(Error("a comment has a control character".to_string()));
    }
    Ok(())
}
fn check_name(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(Error(format!("the {label} of a comment is empty")));
    }
    if value.chars().count() > MAX_NAME || value.chars().any(char::is_control) {
        return Err(Error(format!("the {label} of a comment is too long or has a control character")));
    }
    Ok(())
}
fn check_date(value: &str) -> Result<()> {
    let b = value.as_bytes();
    let ok = b.len() == 19 && b.iter().enumerate().all(|(i, c)| match i {
        4 | 7 => *c == b'-',
        10 => *c == b'T',
        13 | 16 => *c == b':',
        _ => c.is_ascii_digit(),
    });
    if ok {
        Ok(())
    } else {
        Err(Error("the date must look like 2026-11-11T21:30:34".to_string()))
    }
}

/// Add the comments part (relationship and content type) when the package has none, then write `parsed` into it.
fn write_back(package: &Package, parsed: &Parsed) -> Result<Vec<u8>> {
    let mut extras: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    extras.insert(PART.to_string(), serialize(parsed).into_bytes());
    if !package.contains(PART) {
        let mut rels = package.text(DOC_RELS)?.to_string();
        let mut n = 1;
        while rels.contains(&format!("Id='rId{n}'")) || rels.contains(&format!("Id=\"rId{n}\"")) {
            n += 1;
        }
        let at = rels.rfind("</Relationships>").ok_or_else(|| Error("bad visio/_rels/document.xml.rels".to_string()))?;
        rels.insert_str(at, &format!("<Relationship Id='rId{n}' Type='{REL_TYPE}' Target='comments.xml'/>"));
        extras.insert(DOC_RELS.to_string(), rels.into_bytes());
        let mut types = package.text(TYPES)?.to_string();
        let at = types.rfind("</Types>").ok_or_else(|| Error("bad [Content_Types].xml".to_string()))?;
        types.insert_str(at, &format!("<Override PartName='/{PART}' ContentType='{CONTENT_TYPE}'/>"));
        extras.insert(TYPES.to_string(), types.into_bytes());
    }
    write_package_edit(package, extras, &[])
}

fn next_id<'a>(ids: impl Iterator<Item = &'a str>) -> u64 {
    ids.filter_map(|v| v.parse::<u64>().ok()).max().unwrap_or(0) + 1
}

/// Add a comment by `author` (`initials`) to page `page_index`, attached to shape `shape_id` or, with `None`, to the page itself.
/// `now` is the local time `YYYY-MM-DDTHH:MM:SS`. Returns the new package bytes and the new comment's ID.
pub fn add_comment(package: &Package, page_index: usize, shape_id: Option<&str>, author: &str, initials: &str, text: &str, now: &str) -> Result<(Vec<u8>, String)> {
    check_text(text)?;
    check_name("author", author)?;
    check_name("initials", initials)?;
    check_date(now)?;
    let pages = package.pages()?;
    let page = pages.get(page_index).ok_or_else(|| Error(format!("page {page_index} does not exist")))?;
    let page_id = page.id.clone().ok_or_else(|| Error("the page has no ID".to_string()))?;
    if let Some(shape) = shape_id {
        let xml = package.text(&page.part)?;
        if find_shape_range(xml, shape).is_none() {
            return Err(Error(format!("shape {shape} not found on page {page_index}")));
        }
    }
    let mut parsed = match load(package)? {
        Some(p) => p,
        None => Parsed { root_open: ROOT_OPEN.to_string(), authors: Vec::new(), entries: Vec::new() },
    };
    if parsed.entries.len() >= MAX_COMMENTS {
        return Err(Error(format!("a drawing can have at most {MAX_COMMENTS} comments")));
    }
    let author_id = match parsed.authors.iter().find(|a| a.get("Name") == author) {
        Some(a) => a.id.clone(),
        None => {
            let id = next_id(parsed.authors.iter().map(|a| a.id.as_str())).to_string();
            let resolution = format!("<resolutionId provider=\"None\"><localId name=\"{}\"/></resolutionId>", escape_attr(author));
            parsed.authors.push(Author { id: id.clone(), attrs: vec![("Name".into(), author.to_string()), ("Initials".into(), initials.to_string()), ("ResolutionID".into(), resolution), ("ID".into(), id.clone())] });
            id
        }
    };
    let id = next_id(parsed.entries.iter().filter_map(|e| e.get("CommentID"))).to_string();
    let mut attrs = vec![("AuthorID".to_string(), author_id), ("PageID".to_string(), page_id)];
    if let Some(shape) = shape_id {
        attrs.push(("ShapeID".to_string(), shape.to_string()));
    }
    attrs.extend([("Date".to_string(), now.to_string()), ("EditDate".to_string(), now.to_string()), ("CommentID".to_string(), id.clone()), ("AutoCommentType".to_string(), "0".to_string())]);
    parsed.entries.push(Entry { attrs, text: text.to_string() });
    Ok((write_back(package, &parsed)?, id))
}

/// Change the text of comment `id`; its edit date becomes `now`.
pub fn set_comment_text(package: &Package, id: &str, text: &str, now: &str) -> Result<Vec<u8>> {
    check_text(text)?;
    check_date(now)?;
    let mut parsed = load(package)?.ok_or_else(|| Error("the drawing has no comments".to_string()))?;
    let entry = parsed.entries.iter_mut().find(|e| e.get("CommentID") == Some(id)).ok_or_else(|| Error(format!("no comment {id}")))?;
    entry.text = text.to_string();
    entry.set("EditDate", now);
    write_back(package, &parsed)
}

/// Delete comment `id` (its author stays in the author list, as Visio keeps it).
pub fn delete_comment(package: &Package, id: &str) -> Result<Vec<u8>> {
    let mut parsed = load(package)?.ok_or_else(|| Error("the drawing has no comments".to_string()))?;
    let before = parsed.entries.len();
    parsed.entries.retain(|e| e.get("CommentID") != Some(id));
    if parsed.entries.len() == before {
        return Err(Error(format!("no comment {id}")));
    }
    write_back(package, &parsed)
}
