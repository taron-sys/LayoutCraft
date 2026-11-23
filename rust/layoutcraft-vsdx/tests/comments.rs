//! Comments in Visio's form (probe-comments.ps1): one part `visio/comments.xml` with an author list and comment entries, a relationship from the
//! document and a content type; read from a Visio-authored file (test/corpus/comments.vsdx), written, edited and deleted.
#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use std::path::PathBuf;

use layoutcraft_vsdx::comments::{add_comment, delete_comment, list_comments, set_comment_text};
use layoutcraft_vsdx::draw::{draw_shape, DrawKind};
use layoutcraft_vsdx::pageops::add_page;
use layoutcraft_vsdx::{write_blank_document, BlankOptions, Package};

const NOW: &str = "2026-11-11T21:30:34";

fn pkg(bytes: Vec<u8>) -> Package {
    Package::from_bytes(&bytes).unwrap()
}

/// A blank two-page drawing ("Main", "Second page") with one rectangle on page 0.
fn drawing() -> (Package, String) {
    let bytes = write_blank_document(&BlankOptions { width_in: 8.5, height_in: 11.0, metric: false, name: "Main".to_string() }).unwrap();
    let p = pkg(bytes);
    let (b, _) = add_page(&p, 0, Some("Second page")).unwrap();
    let p = pkg(b);
    let (b, id) = draw_shape(&p, 0, DrawKind::Rectangle, 1.0, 1.0, 3.0, 2.0).unwrap();
    (pkg(b), id)
}

#[test]
fn a_visio_authored_file_is_read() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/comments.vsdx");
    let p = Package::from_bytes(&std::fs::read(path).unwrap()).unwrap();
    let list = list_comments(&p).unwrap();
    assert_eq!(list.len(), 5);
    assert_eq!(list[0].text, "First comment on a shape & more <tag>", "the XML escaping is undone");
    assert_eq!(list[1].text, "Second comment\nwith two lines", "newlines are kept");
    assert_eq!((list[0].shape_id.as_deref(), list[0].page_index), (Some("1"), Some(0)));
    assert_eq!((list[2].shape_id.as_deref(), list[2].page_index), (Some("2"), Some(0)));
    assert_eq!((list[3].shape_id.clone(), list[3].page_index), (None, Some(0)), "a comment on the page itself has no shape");
    assert_eq!((list[4].shape_id.clone(), list[4].page_index), (None, Some(1)), "PageID 4 is the second page, not index 4");
    assert_eq!(list[0].initials.len(), 1);
    assert!(!list[0].author.is_empty() && list[0].date.len() == 19 && list[0].edit_date.len() == 19);
}

#[test]
fn a_new_comment_creates_the_part_the_relationship_and_the_content_type() {
    let (p, shape) = drawing();
    assert!(list_comments(&p).unwrap().is_empty() && !p.contains("visio/comments.xml"));
    let (b, id) = add_comment(&p, 0, Some(&shape), "Ann Example", "AE", "Check the size & the <colour>", NOW).unwrap();
    let p = pkg(b);
    assert_eq!(id, "1");
    assert!(p.contains("visio/comments.xml"));
    assert!(p.text("[Content_Types].xml").unwrap().contains("<Override PartName='/visio/comments.xml' ContentType='application/vnd.ms-visio.comments+xml'/>"));
    let rels = p.text("visio/_rels/document.xml.rels").unwrap();
    assert!(rels.contains("Type='http://schemas.microsoft.com/visio/2010/relationships/comments' Target='comments.xml'"), "{rels}");
    let xml = p.text("visio/comments.xml").unwrap().to_string();
    assert!(xml.contains("<Comments ShowCommentTags='1' xmlns='http://schemas.microsoft.com/office/visio/2012/main'"), "{xml}");
    assert!(xml.contains("<AuthorEntry Name='Ann Example' Initials='AE' ResolutionID='"), "{xml}");
    assert!(xml.contains(&format!("<CommentEntry AuthorID='1' PageID='0' ShapeID='{shape}' Date='{NOW}' EditDate='{NOW}' CommentID='1' AutoCommentType='0'>Check the size &amp; the &lt;colour&gt;</CommentEntry>")), "{xml}");
    let list = list_comments(&p).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!((list[0].author.as_str(), list[0].initials.as_str(), list[0].page_index, list[0].shape_id.as_deref()), ("Ann Example", "AE", Some(0), Some(shape.as_str())));
}

#[test]
fn authors_are_shared_ids_count_up_and_page_comments_have_no_shape() {
    let (p, shape) = drawing();
    let (b, first) = add_comment(&p, 0, Some(&shape), "Ann", "A", "one", NOW).unwrap();
    let p = pkg(b);
    let (b, second) = add_comment(&p, 1, None, "Ann", "A", "on the second page\nin two lines", "2026-11-12T08:00:00").unwrap();
    let p = pkg(b);
    let (b, third) = add_comment(&p, 0, None, "Bob", "B", "by someone else", NOW).unwrap();
    let p = pkg(b);
    assert_eq!((first.as_str(), second.as_str(), third.as_str()), ("1", "2", "3"));
    let xml = p.text("visio/comments.xml").unwrap().to_string();
    assert_eq!(xml.matches("<AuthorEntry").count(), 2, "Ann is listed once: {xml}");
    let list = list_comments(&p).unwrap();
    assert_eq!(list[1].page_index, Some(1));
    assert_eq!(list[1].shape_id, None);
    assert_eq!(list[1].text, "on the second page\nin two lines");
    assert_eq!(list[2].author, "Bob");
    assert_eq!(list.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["1", "2", "3"]);
}

#[test]
fn edit_and_delete() {
    let (p, shape) = drawing();
    let (b, id) = add_comment(&p, 0, Some(&shape), "Ann", "A", "draft", NOW).unwrap();
    let p = pkg(b);
    let (b, other) = add_comment(&p, 0, None, "Ann", "A", "keep me", NOW).unwrap();
    let p = pkg(b);
    let p = pkg(set_comment_text(&p, &id, "final text", "2026-11-13T09:15:00").unwrap());
    let list = list_comments(&p).unwrap();
    assert_eq!((list[0].text.as_str(), list[0].date.as_str(), list[0].edit_date.as_str()), ("final text", NOW, "2026-11-13T09:15:00"), "only the edit date moves");
    let p = pkg(delete_comment(&p, &id).unwrap());
    let list = list_comments(&p).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, other);
    let (b, next) = add_comment(&p, 0, None, "Ann", "A", "after the delete", NOW).unwrap();
    assert_eq!(next, "3", "ids are never reused while a larger one exists");
    assert!(list_comments(&pkg(b)).unwrap().len() == 2);
    assert!(delete_comment(&p, "99").is_err());
    assert!(set_comment_text(&p, "99", "x", NOW).is_err());
}

#[test]
fn bad_input_is_refused() {
    let (p, shape) = drawing();
    assert!(add_comment(&p, 0, Some(&shape), "Ann", "A", "   ", NOW).is_err(), "no empty comment");
    assert!(add_comment(&p, 0, Some(&shape), "Ann", "A", "a\u{1}b", NOW).is_err(), "no control characters");
    assert!(add_comment(&p, 0, Some(&shape), "Ann", "A", &"x".repeat(40_000), NOW).is_err(), "no endless text");
    assert!(add_comment(&p, 0, Some(&shape), "", "A", "text", NOW).is_err(), "an author is needed");
    assert!(add_comment(&p, 0, Some(&shape), "Ann", "A", "text", "yesterday").is_err(), "a date in Visio's form");
    assert!(add_comment(&p, 0, Some("999"), "Ann", "A", "text", NOW).is_err(), "an unknown shape");
    assert!(add_comment(&p, 5, None, "Ann", "A", "text", NOW).is_err(), "an unknown page");
}

#[test]
fn comments_survive_a_round_trip_through_the_package_writer() {
    let (p, shape) = drawing();
    let (b, _) = add_comment(&p, 0, Some(&shape), "Ann", "A", "stays", NOW).unwrap();
    let p = pkg(b);
    let (b, _) = draw_shape(&p, 0, DrawKind::Ellipse, 4.0, 4.0, 5.0, 5.0).unwrap(); // another edit of the package keeps the part
    let p = pkg(b);
    assert_eq!(list_comments(&p).unwrap().len(), 1);
    assert_eq!(list_comments(&p).unwrap()[0].text, "stays");
}
