//! The themes that ship with the public source: three themes invented for LayoutCraft (own colours, own fonts, own names).
//!
//! Further themes are NOT part of the public source tree. They live in the git-ignored folder
//! `private/themes-extra/` and are compiled in only when that folder exists (`build.rs` sets `cfg(private_themes)`); see `themes.rs`.
//!
//! The theme part is written from the compact tree notation of `themegen.rs`; `own_dsl` builds it from six accent colours and a font.

/// (name, gallery index, six accent colours without `#`, font). Indices start at 101, away from the low numbers the private extra themes use.
pub const OWN_THEMES: [(&str, u32, [&str; 6], &str); 3] = [
    ("Daylight", 101, ["3A7BD5", "F29E4C", "56B870", "E4572E", "8E6BBF", "17A2B8"], "Arial"),
    ("Dusk", 102, ["5B5EA6", "C94C7B", "3FA7A0", "E0A33A", "7A8B99", "9C6644"], "Georgia"),
    ("Meadow", 103, ["4C9F70", "A3B18A", "D4A373", "6B8F71", "BC6C25", "3D5A80"], "Verdana"),
];

fn clr(tag: &str, hex: &str) -> String {
    format!("a:{tag}{{a:srgbClr(val='{hex}');}}")
}

/// The theme tree of one own theme.
pub fn own_dsl(name: &str, index: u32, accents: &[&str; 6], font: &str) -> String {
    let guid = "{00000000-0000-0000-0000-000000000000}";
    let mut s = format!("a:themeElements{{a:clrScheme(name='{name}'){{");
    s += &clr("dk1", "000000");
    s += &clr("lt1", "FFFFFF");
    s += &clr("dk2", "3B4252");
    s += &clr("lt2", "ECEFF4");
    for (i, c) in accents.iter().enumerate() {
        s += &clr(&format!("accent{}", i + 1), c);
    }
    s += &clr("hlink", "0563C1");
    s += &clr("folHlink", "954F72");
    s += &format!("a:extLst{{a:ext(uri='{{2703A3B3-D2E1-43D9-8057-6E9D74E0F44A}}'){{vt:schemeID(schemeEnum='{index}',schemeGUID='{guid}');}}");
    s += "a:ext(uri='{093E89EA-6996-430E-BFF9-83A9FAAAAB73}'){vt:bkgnd{a:srgbClr(val='FFFFFF');}}";
    s += "a:ext(uri='{DDD2D869-C2EF-471E-B8FA-914AFA308C9F}'){vt:variationClrSchemeLst{";
    // four colour variations: the accents rotated by one place each, the seventh colour is the second dark colour
    for k in 0..4 {
        s += "vt:variationClrScheme{";
        for j in 0..7 {
            let hex = if j < 6 { accents[(j + k) % 6] } else { "3B4252" };
            s += &format!("vt:varColor{}{{a:srgbClr(val='{hex}');}}", j + 1);
        }
        s += "}";
    }
    s += "}}}}"; // variationClrSchemeLst, ext, extLst, clrScheme
    s += &format!("a:fontScheme(name='{name}'){{a:majorFont{{a:latin(typeface='{font}');a:ea(typeface='');a:cs(typeface='');}}a:minorFont{{a:latin(typeface='{font}');a:ea(typeface='');a:cs(typeface='');}}");
    s += &format!("a:extLst{{a:ext(uri='{{B3C0788F-89CC-4CDB-A57D-0664CFDA821B}}'){{vt:schemeID(schemeEnum='{index}',schemeGUID='{guid}');}}}}}}");
    s += &format!("a:fmtScheme(name='{name}'){{");
    let solid = "a:solidFill{a:schemeClr(val='phClr');}";
    s += &format!("a:fillStyleLst{{{}}}", solid.repeat(6));
    let line = "a:ln(w='12700',cap='flat',cmpd='sng',algn='ctr'){a:solidFill{a:schemeClr(val='phClr');}a:prstDash(val='solid');a:miter(lim='800000');}";
    s += &format!("a:lnStyleLst{{{}}}", line.repeat(6));
    s += &format!("a:effectStyleLst{{{}}}", "a:effectStyle{a:effectLst;}".repeat(6));
    s += &format!("a:bgFillStyleLst{{{}}}", solid.repeat(6));
    s += "a:extLst{a:ext(uri='{494CE47F-D151-47DC-95E8-85652EA8A67E}'){vt:variationStyleSchemeLst{vt:variationStyleScheme(embellishment='0'){";
    s += &"vt:varStyle(fillIdx='1',lineIdx='1',effectIdx='1',fontIdx='1');".repeat(4);
    s += "}}}}"; // variationStyleScheme, list, ext, extLst
    s += "}}"; // fmtScheme, themeElements
    s += "a:objectDefaults;a:extraClrSchemeLst;";
    s
}
