//! The OLE compound file container, as far as reading one named stream needs it.
//!
//! Evidence: the public specification [MS-CFB] (header, FAT, DIFAT, mini stream and directory entries).
use crate::{Error, Result};

pub(super) const MAGIC: [u8; 8] = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];
/// Sector numbers from here up are markers (end of chain, free, FAT and DIFAT sectors).
const MARKERS: u32 = 0xffff_fffa;
const DIRECTORY_ENTRY: usize = 128;

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(data.get(at..at.checked_add(2)?)?.try_into().ok()?))
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(at..at.checked_add(4)?)?.try_into().ok()?))
}

fn damaged(what: &str) -> Error {
    Error(format!("damaged compound file: {what}"))
}

/// The bytes of the stream called `name` (any storage level; the first one wins).
pub(super) fn stream(data: &[u8], name: &str) -> Result<Vec<u8>> {
    let sector_shift = u16_at(data, 30).ok_or_else(|| damaged("short header"))?;
    let mini_shift = u16_at(data, 32).ok_or_else(|| damaged("short header"))?;
    if !(sector_shift == 9 || sector_shift == 12) || mini_shift != 6 {
        return Err(damaged("unsupported sector size"));
    }
    let size = 1usize << sector_shift;
    let header = |at: usize| u32_at(data, at).ok_or_else(|| damaged("short header"));
    let fat_count = header(44)? as usize;
    let directory_start = header(48)?;
    let cutoff = header(56)? as u64;
    let mini_fat_start = header(60)?;
    let mut difat_next = header(68)?;
    let sectors = data.len() / size;
    // A sector may be cut short at the end of the file.
    let sector = |n: u32| -> Option<&[u8]> {
        let from = (n as usize).checked_add(1)?.checked_mul(size)?;
        if from >= data.len() {
            return None;
        }
        Some(&data[from..data.len().min(from + size)])
    };
    if fat_count > sectors + 1 {
        return Err(damaged("more FAT sectors than the file holds"));
    }
    let mut fat_sectors = Vec::new();
    for index in 0..109 {
        fat_sectors.push(header(76 + index * 4)?);
    }
    let mut seen = 0usize;
    while difat_next < MARKERS && fat_sectors.len() < fat_count {
        let block = sector(difat_next).ok_or_else(|| damaged("DIFAT outside the file"))?;
        for index in 0..size / 4 - 1 {
            fat_sectors.push(u32_at(block, index * 4).unwrap_or(u32::MAX));
        }
        difat_next = u32_at(block, size - 4).unwrap_or(u32::MAX);
        seen += 1;
        if seen > sectors {
            return Err(damaged("DIFAT loop"));
        }
    }
    let mut fat = Vec::new();
    for &n in fat_sectors.iter().take(fat_count) {
        let block = sector(n).ok_or_else(|| damaged("FAT outside the file"))?;
        fat.extend((0..block.len() / 4).filter_map(|i| u32_at(block, i * 4)));
    }
    // Follow a chain through an allocation table; a chain can never be longer than the table.
    let chain = |table: &[u32], start: u32| -> Result<Vec<u32>> {
        let mut out = Vec::new();
        let mut at = start;
        while at < MARKERS {
            if out.len() > table.len() {
                return Err(damaged("sector chain loop"));
            }
            out.push(at);
            at = *table.get(at as usize).ok_or_else(|| damaged("sector chain leaves the table"))?;
        }
        Ok(out)
    };
    let read = |start: u32, limit: usize| -> Result<Vec<u8>> {
        let mut out = Vec::new();
        for n in chain(&fat, start)? {
            out.extend_from_slice(sector(n).ok_or_else(|| damaged("stream outside the file"))?);
            if out.len() >= limit {
                break;
            }
            // A looping chain must not grow past the file.
            if out.len() > data.len() {
                return Err(damaged("stream longer than the file"));
            }
        }
        out.truncate(limit);
        Ok(out)
    };
    let directory = read(directory_start, data.len())?;
    let wanted: Vec<u8> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let entry = directory
        .chunks_exact(DIRECTORY_ENTRY)
        .find(|entry| {
            let length = u16_at(entry, 64).unwrap_or(0) as usize;
            entry[66] == 2 && length == wanted.len() + 2 && entry[..wanted.len()] == wanted[..]
        })
        .ok_or_else(|| Error(format!("the file has no {name} stream: it is not a Visio drawing")))?;
    let start = u32_at(entry, 116).ok_or_else(|| damaged("directory entry"))?;
    let length = u32_at(entry, 120).ok_or_else(|| damaged("directory entry"))? as u64;
    if length > data.len() as u64 {
        return Err(damaged("stream longer than the file"));
    }
    let length = length as usize;
    if length as u64 >= cutoff {
        let out = read(start, length)?;
        return if out.len() == length { Ok(out) } else { Err(damaged("truncated stream")) };
    }
    // Short streams live in 64-byte sectors inside the root entry's stream.
    let root_start = u32_at(&directory, 116).ok_or_else(|| damaged("no root entry"))?;
    let mini_stream = read(root_start, data.len())?;
    let table_bytes = if mini_fat_start < MARKERS { read(mini_fat_start, data.len())? } else { Vec::new() };
    let table: Vec<u32> = (0..table_bytes.len() / 4).filter_map(|i| u32_at(&table_bytes, i * 4)).collect();
    let mut out = Vec::with_capacity(length);
    for n in chain(&table, start)? {
        let from = (n as usize) * 64;
        out.extend_from_slice(mini_stream.get(from..from + 64).ok_or_else(|| damaged("short stream outside its container"))?);
        if out.len() >= length {
            break;
        }
    }
    if out.len() < length {
        return Err(damaged("truncated stream"));
    }
    out.truncate(length);
    Ok(out)
}
