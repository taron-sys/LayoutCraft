//! Document properties (File > Info > Properties in Visio): Title, Subject, Author, Keywords, Comments, Category in
//! `docProps/core.xml`, Manager and Company in `docProps/app.xml` (measured with `probe-docprops.ps1`: Visio's
//! `Document.Title/Subject/Creator/Keywords/Description/Category` land in core.xml as `dc:title`, `dc:subject`,
//! `dc:creator`, `cp:keywords`, `dc:description`, `cp:category`; `Manager` and `Company` in app.xml).
//! Only the elements of the eight properties are touched; everything else in both parts is kept as it is.

use std::collections::BTreeMap;

use crate::writer::write_package_edit;
use crate::{Error, Package, Result};

const CORE: &str = "docProps/core.xml";
const APP: &str = "docProps/app.xml";
const RELS: &str = "_rels/.rels";
const TYPES: &str = "[Content_Types].xml";
const CORE_TYPE: &str = "application/vnd.openxmlformats-package.core-properties+xml";
const APP_TYPE: &str = "application/vnd.openxmlformats-officedocument.extended-properties+xml";
const CORE_REL: &str = "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";
const APP_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties";
const MAX_LEN: usize = 4096;

/// The eight properties of the dialog. Empty = not set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocProps {
    pub title: String,
    pub subject: String,
    pub creator: String,
    pub keywords: String,
    pub description: String,
    pub category: String,
    pub manager: String,
    pub company: String,
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// The span of element `tag` (e.g. `dc:title`): (start of the open tag, end of the close tag, content start, content end);
/// a self-closing element has an empty content.
fn element(xml: &str, tag: &str) -> Option<(usize, usize, usize, usize)> {
    let open = format!("<{tag}");
    let mut from = 0;
    while let Some(found) = xml[from..].find(&open) {
        let start = from + found;
        let after = start + open.len();
        let next = xml[after..].chars().next()?;
        if next == '>' || next == '/' || next.is_whitespace() {
            let gt = after + xml[after..].find('>')?;
            if xml[..gt].ends_with('/') {
                return Some((start, gt + 1, gt, gt));
            }
            let close = format!("</{tag}>");
            let end = gt + 1 + xml[gt + 1..].find(&close)?;
            return Some((start, end + close.len(), gt + 1, end));
        }
        from = after;
    }
    None
}

fn get(xml: &str, tag: &str) -> String {
    element(xml, tag).map(|(_, _, a, b)| unescape(xml[a..b].trim())).unwrap_or_default()
}

/// Read the properties of an open package.
pub fn read_doc_props(package: &Package) -> DocProps {
    let core = package.text(CORE).unwrap_or("");
    let app = package.text(APP).unwrap_or("");
    DocProps {
        title: get(core, "dc:title"),
        subject: get(core, "dc:subject"),
        creator: get(core, "dc:creator"),
        keywords: get(core, "cp:keywords"),
        description: get(core, "dc:description"),
        category: get(core, "cp:category"),
        manager: get(app, "Manager"),
        company: get(app, "Company"),
    }
}

/// Set (or, for an empty value in core.xml, remove) one element, keeping the rest of the part.
fn set(xml: &mut String, tag: &str, value: &str, keep_empty: bool) -> Result<()> {
    let new = if value.is_empty() && !keep_empty { String::new() } else { format!("<{tag}>{}</{tag}>", escape(value)) };
    if let Some((start, end, _, _)) = element(xml, tag) {
        xml.replace_range(start..end, &new);
    } else if !new.is_empty() {
        let root_end = xml.rfind("</").ok_or_else(|| Error(format!("a properties part has no closing tag for {tag}")))?;
        xml.insert_str(root_end, &new);
    }
    Ok(())
}

fn check(props: &DocProps) -> Result<()> {
    for (name, value) in [("title", &props.title), ("subject", &props.subject), ("author", &props.creator), ("keywords", &props.keywords), ("comments", &props.description), ("category", &props.category), ("manager", &props.manager), ("company", &props.company)] {
        if value.chars().count() > MAX_LEN {
            return Err(Error(format!("the {name} is longer than {MAX_LEN} characters")));
        }
        if value.chars().any(|c| c.is_control() && c != '\n' && c != '\r' && c != '\t') {
            return Err(Error(format!("the {name} contains a control character")));
        }
    }
    Ok(())
}

/// A new package with `props` written; parts that do not exist yet are created (with relationship and content type).
pub fn write_doc_props(package: &Package, props: &DocProps) -> Result<Vec<u8>> {
    check(props)?;
    let mut extras: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut rels = package.text(RELS).map(str::to_string).unwrap_or_default();
    let mut types = package.text(TYPES)?.to_string();
    let mut touched_package_parts = false;

    let mut core = match package.text(CORE) {
        Ok(text) => text.to_string(),
        Err(_) => {
            touched_package_parts = true;
            add_part(&mut rels, &mut types, CORE, "rIdVlCore", CORE_REL, CORE_TYPE)?;
            "<?xml version='1.0' encoding='UTF-8' standalone='yes'?><cp:coreProperties xmlns:cp='http://schemas.openxmlformats.org/package/2006/metadata/core-properties' xmlns:dc='http://purl.org/dc/elements/1.1/' xmlns:dcterms='http://purl.org/dc/terms/' xmlns:xsi='http://www.w3.org/2001/XMLSchema-instance'></cp:coreProperties>".to_string()
        }
    };
    if !core.contains("xmlns:dc=") || !core.contains("xmlns:cp=") {
        return Err(Error("docProps/core.xml does not declare the standard prefixes".to_string()));
    }
    set(&mut core, "dc:title", &props.title, false)?;
    set(&mut core, "dc:subject", &props.subject, false)?;
    set(&mut core, "dc:creator", &props.creator, false)?;
    set(&mut core, "cp:keywords", &props.keywords, false)?;
    set(&mut core, "dc:description", &props.description, false)?;
    set(&mut core, "cp:category", &props.category, false)?;
    extras.insert(CORE.to_string(), core.into_bytes());

    let mut app = match package.text(APP) {
        Ok(text) => text.to_string(),
        Err(_) => {
            touched_package_parts = true;
            add_part(&mut rels, &mut types, APP, "rIdVlApp", APP_REL, APP_TYPE)?;
            "<?xml version='1.0' encoding='UTF-8' standalone='yes'?><Properties xmlns='http://schemas.openxmlformats.org/officeDocument/2006/extended-properties'><Application>LayoutCraft</Application></Properties>".to_string()
        }
    };
    set(&mut app, "Manager", &props.manager, true)?;
    set(&mut app, "Company", &props.company, true)?;
    extras.insert(APP.to_string(), app.into_bytes());

    if touched_package_parts {
        extras.insert(RELS.to_string(), rels.into_bytes());
        extras.insert(TYPES.to_string(), types.into_bytes());
    }
    write_package_edit(package, extras, &[])
}

fn add_part(rels: &mut String, types: &mut String, part: &str, id: &str, rel_type: &str, content_type: &str) -> Result<()> {
    if rels.is_empty() {
        *rels = "<?xml version='1.0' encoding='UTF-8' standalone='yes'?><Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships'></Relationships>".to_string();
    }
    let at = rels.rfind("</Relationships>").ok_or_else(|| Error("bad _rels/.rels".to_string()))?;
    rels.insert_str(at, &format!("<Relationship Id='{id}' Type='{rel_type}' Target='{part}'/>"));
    let at = types.rfind("</Types>").ok_or_else(|| Error("bad [Content_Types].xml".to_string()))?;
    types.insert_str(at, &format!("<Override PartName='/{part}' ContentType='{content_type}'/>"));
    Ok(())
}
