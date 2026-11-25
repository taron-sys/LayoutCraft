//! The mutation sweeps: fixed seed, fixed counts (times `LAYOUTCRAFT_HOSTILE_SCALE`), every case through the guarded runner.
use crate::hostile::{honest, mutate_attribute, mutate_bytes, mutate_json, mutate_xml, seed, scale, write_zip, Entry, Rng, Sweep};
use crate::{pipeline, seeds};

/// Truncate every package at every `STEP` bytes (and at the bytes around the end), open and exercise what opens.
#[test]
fn packages_cut_off_anywhere_never_panic() {
    const STEP: usize = 97;
    let mut sweep = Sweep::new("truncate");
    for (name, bytes) in seeds::fixtures() {
        let mut cut = 0;
        while cut < bytes.len() {
            sweep.run(&format!("{name} cut at {cut}"), bytes[..cut].to_vec(), pipeline::vsdx);
            cut += STEP;
        }
        for back in 1..=24 {
            sweep.run(&format!("{name} cut {back} before the end"), bytes[..bytes.len() - back].to_vec(), pipeline::vsdx);
        }
    }
    pipeline::report(sweep.name);
    sweep.finish();
}

/// Byte-level damage to the whole file: flips, runs, interesting numbers, cuts, deleted and repeated ranges.
#[test]
fn packages_with_damaged_bytes_never_panic() {
    let fixtures = seeds::fixtures();
    let mut rng = Rng::new(seed());
    let mut sweep = Sweep::new("bytes");
    for n in 0..500 * scale() {
        let (name, original) = rng.pick(&fixtures).clone();
        let mut bytes = original;
        for _ in 0..1 + rng.below(3) {
            mutate_bytes(&mut rng, &mut bytes);
        }
        sweep.run(&format!("#{n} {name}"), bytes, pipeline::vsdx);
    }
    pipeline::report(sweep.name);
    sweep.finish();
}

/// The structure of the ZIP: entries removed, duplicated, renamed, emptied, with sizes and checksums that lie, hostile names.
#[test]
fn zip_structure_damage_never_panics() {
    let fixtures = seeds::fixtures();
    let mut rng = Rng::new(seed() ^ 1);
    let mut sweep = Sweep::new("zip");
    let hostile_names = ["../evil.xml", "/abs.xml", "visio\\pages\\page1.xml", "visio/../../x", "a//b", "", "visio/pages/page1.xml/", "visio/./pages/page1.xml", "CON", "visio/pages/page\u{0}1.xml", "visio/pages/PAGE1.XML"];
    for n in 0..500 * scale() {
        let (name, original) = rng.pick(&fixtures).clone();
        let parts = seeds::parts(&original);
        let mut entries: Vec<Entry> = parts.iter().map(|(n, d)| honest(n, d, rng.chance(2))).collect();
        let what;
        match rng.below(10) {
            0 => {
                let at = rng.below(entries.len());
                what = format!("removed {}", entries[at].name);
                entries.remove(at);
            }
            1 => {
                let at = rng.below(entries.len());
                what = format!("duplicated {}", entries[at].name);
                let mut copy = entries[at].clone();
                if rng.chance(2) {
                    copy = honest(&copy.name, b"<x/>", false);
                }
                entries.insert(rng.below(entries.len()), copy);
            }
            2 => {
                let at = rng.below(entries.len());
                what = format!("renamed {}", entries[at].name);
                entries[at].name = rng.pick(&hostile_names).to_string();
            }
            3 => {
                let at = rng.below(entries.len());
                what = format!("emptied {}", entries[at].name);
                entries[at] = honest(&entries[at].name, b"", rng.chance(2));
            }
            4 => {
                let at = rng.below(entries.len());
                what = format!("declared size of {}", entries[at].name);
                entries[at].size = *rng.pick(&[0, 1, 0x7FFF_FFFF, 0xFFFF_FFFF, 0x1000_0000, entries[at].size.wrapping_add(1), entries[at].size.saturating_sub(1)]);
            }
            5 => {
                let at = rng.below(entries.len());
                what = format!("checksum of {}", entries[at].name);
                entries[at].crc ^= 0xFFFF;
            }
            6 => {
                let at = rng.below(entries.len());
                what = format!("garbled data of {}", entries[at].name);
                mutate_bytes(&mut rng, &mut entries[at].stored);
            }
            7 => {
                what = "an unknown method".to_string();
                let at = rng.below(entries.len());
                entries[at].method = *rng.pick(&[1, 6, 9, 12, 14, 93, 95, 99]);
            }
            8 => {
                what = "extra entries".to_string();
                for k in 0..1 + rng.below(40) {
                    entries.push(honest(&format!("visio/pages/page{}.xml", 100 + k), b"<PageContents/>", true));
                }
            }
            _ => {
                what = "shuffled".to_string();
                for i in (1..entries.len()).rev() {
                    let j = rng.below(i + 1);
                    entries.swap(i, j);
                }
            }
        }
        sweep.run(&format!("#{n} {name}: {what}"), write_zip(&entries), pipeline::vsdx);
    }
    pipeline::report(sweep.name);
    sweep.finish();
}

/// The XML of the parts: hostile attribute values, cut elements, repeated and removed markup. The share that opens is exercised in full.
#[test]
fn damaged_xml_parts_never_panic() {
    let fixtures = seeds::fixtures();
    let mut rng = Rng::new(seed() ^ 2);
    let mut sweep = Sweep::new("xml");
    for n in 0..500 * scale() {
        let (name, original) = rng.pick(&fixtures).clone();
        let mut parts = seeds::parts(&original);
        let xml: Vec<usize> = (0..parts.len()).filter(|i| parts[*i].0.ends_with(".xml") || parts[*i].0.ends_with(".rels")).collect();
        let mut what = String::new();
        for _ in 0..1 + rng.below(3) {
            // pages and document are the parts the importer cares about most
            let at = if rng.chance(2) { *xml.iter().find(|i| parts[**i].0.contains("page1")).unwrap_or(&xml[0]) } else { *rng.pick(&xml) };
            let text = String::from_utf8_lossy(&parts[at].1).into_owned();
            parts[at].1 = mutate_xml(&mut rng, &text).into_bytes();
            what = format!("{} {}", what, parts[at].0);
        }
        let run: fn(&[u8]) = if n % 40 == 0 { pipeline::vsdx_and_edit } else { pipeline::vsdx };
        sweep.run(&format!("#{n} {name}:{what}"), seeds::zip(&parts), run);
    }
    pipeline::report(sweep.name);
    sweep.finish();
}

/// The unmutated fixtures go through everything, edits included (the baseline the mutants are compared with).
#[test]
fn the_public_fixtures_go_through_every_path() {
    let mut sweep = Sweep::new("fixtures");
    for (name, bytes) in seeds::fixtures() {
        sweep.run(&name, bytes, pipeline::vsdx_and_edit);
    }
    pipeline::report(sweep.name);
    sweep.finish();
}

#[test]
fn damaged_stencils_never_panic() {
    let original = seeds::stencil();
    let parts = seeds::parts(&original);
    let mut rng = Rng::new(seed() ^ 3);
    let mut sweep = Sweep::new("stencil");
    sweep.run("unmutated", original.clone(), pipeline::stencil);
    for n in 0..300 * scale() {
        let mut parts = parts.clone();
        let xml: Vec<usize> = (0..parts.len()).filter(|i| parts[*i].0.ends_with(".xml") || parts[*i].0.ends_with(".rels")).collect();
        let at = *rng.pick(&xml);
        let text = String::from_utf8_lossy(&parts[at].1).into_owned();
        parts[at].1 = mutate_xml(&mut rng, &text).into_bytes();
        let mut bytes = seeds::zip(&parts);
        if rng.chance(4) {
            mutate_bytes(&mut rng, &mut bytes);
        }
        sweep.run(&format!("#{n} {}", parts[at].0), bytes, pipeline::stencil);
    }
    pipeline::report(sweep.name);
    sweep.finish();
}

fn text_sweep(name: &'static str, salt: u64, seeds_: Vec<String>, run: fn(&[u8]), mutate: fn(&mut Rng, &str) -> String) {
    let mut rng = Rng::new(seed() ^ salt);
    let mut sweep = Sweep::new(name);
    for (k, original) in seeds_.iter().enumerate() {
        sweep.run(&format!("seed {k}"), original.clone().into_bytes(), run);
    }
    for n in 0..600 * scale() {
        let mut text = rng.pick(&seeds_).clone();
        for _ in 0..1 + rng.below(3) {
            text = mutate(&mut rng, &text);
        }
        sweep.run(&format!("#{n}"), text.into_bytes(), run);
    }
    // truncation at every 61 bytes
    for original in &seeds_ {
        let mut cut = 0;
        while cut < original.len() {
            let mut end = cut;
            while !original.is_char_boundary(end) {
                end -= 1;
            }
            sweep.run(&format!("cut at {end}"), original.as_bytes()[..end].to_vec(), run);
            cut += 61;
        }
    }
    pipeline::report(sweep.name);
    sweep.finish();
}

#[test]
fn damaged_drawio_files_never_panic() {
    text_sweep("drawio", 4, vec![seeds::DRAWIO.to_string(), seeds::drawio_compressed()], pipeline::drawio, |rng, text| {
        if rng.chance(8) {
            mutate_attribute(rng, text)
        } else {
            mutate_xml(rng, text)
        }
    });
}

#[test]
fn damaged_excalidraw_files_never_panic() {
    text_sweep("excalidraw", 5, vec![seeds::EXCALIDRAW.to_string()], pipeline::excalidraw, |rng, text| mutate_json(rng, text));
}

#[test]
fn damaged_metafiles_never_panic() {
    let mut rng = Rng::new(seed() ^ 6);
    let mut sweep = Sweep::new("metafile");
    let originals = seeds::metafiles();
    for (name, bytes) in &originals {
        sweep.run(name, bytes.clone(), pipeline::metafile);
        let mut cut = 0;
        while cut < bytes.len() {
            sweep.run(&format!("{name} cut at {cut}"), bytes[..cut].to_vec(), pipeline::metafile);
            cut += 7;
        }
    }
    for n in 0..2000 * scale() {
        let (name, original) = rng.pick(&originals).clone();
        let mut bytes = original;
        for _ in 0..1 + rng.below(4) {
            mutate_bytes(&mut rng, &mut bytes);
        }
        sweep.run(&format!("#{n} {name}"), bytes, pipeline::metafile);
    }
    // every record kind with random parameters, after a valid header
    let emf = seeds::emf();
    for n in 0..1500 * scale() {
        let mut bytes = emf[..88].to_vec();
        for _ in 0..1 + rng.below(4) {
            let kind = 1 + rng.below(130) as u32;
            let len = 4 * rng.below(40);
            bytes.extend(kind.to_le_bytes());
            bytes.extend(((len + 8) as u32).to_le_bytes());
            for _ in 0..len {
                bytes.push(if rng.chance(3) { rng.next() as u8 } else { 0 });
            }
        }
        sweep.run(&format!("#{n} random EMF records"), bytes, pipeline::metafile);
    }
    let wmf = seeds::wmf();
    for n in 0..1000 * scale() {
        let mut bytes = wmf[..wmf.len() - 6].to_vec();
        for _ in 0..1 + rng.below(4) {
            let func = *rng.pick(&[0x0324u16, 0x0325, 0x041B, 0x0521, 0x0418, 0x0538, 0x0817, 0x0A32, 0x02FB, 0x01F0, 0x012D, 0x020B, 0x020C, 0x0626]) ^ (rng.below(3) as u16 * 0x100);
            let words = 3 + rng.below(30);
            bytes.extend((words as u32).to_le_bytes());
            bytes.extend(func.to_le_bytes());
            for _ in 0..(words - 3) * 2 {
                bytes.push(if rng.chance(3) { rng.next() as u8 } else { 0 });
            }
        }
        sweep.run(&format!("#{n} random WMF records"), bytes, pipeline::metafile);
    }
    pipeline::report(sweep.name);
    sweep.finish();
}

/// A hostile metafile as the picture part of a drawing: the whole path from package to page SVG and PDF.
#[test]
fn hostile_pictures_inside_packages_never_panic() {
    let fixtures = seeds::fixtures();
    let picture = fixtures.iter().find(|(n, _)| n.contains("picture")).map(|(_, b)| b.clone()).unwrap_or_default();
    let mut rng = Rng::new(seed() ^ 7);
    let mut sweep = Sweep::new("picture");
    let originals = seeds::metafiles();
    for n in 0..150 * scale() {
        let (name, original) = rng.pick(&originals).clone();
        let mut bytes = original;
        mutate_bytes(&mut rng, &mut bytes);
        let mut parts = seeds::parts(&picture);
        // the picture part of the fixture is replaced by the metafile under the same name
        let media = parts.iter().position(|(n, _)| n.starts_with("visio/media/")).unwrap_or(0);
        parts[media].1 = bytes;
        if rng.chance(2) {
            parts[media].0 = "visio/media/layoutcraft1.emf".to_string();
            for (_, data) in parts.iter_mut().filter(|(n, _)| n.ends_with(".rels") || n.ends_with("[Content_Types].xml")) {
                *data = String::from_utf8_lossy(data).replace("layoutcraft1.png", "layoutcraft1.emf").into_bytes();
            }
        }
        sweep.run(&format!("#{n} {name}"), seeds::zip(&parts), pipeline::picture_in_package);
    }
    pipeline::report(sweep.name);
    sweep.finish();
}
