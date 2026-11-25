//! Shared tools of the hostile-input sweeps (included by `#[path]` from the vsdx and the engine test targets, so it
//! stays dependency-free: a fixed-seed PRNG, a ZIP writer that can lie, text mutators and a guarded case runner).
//!
//! The runner runs every case on its own thread (2 MiB stack, the size of a test thread): a panic is caught and its
//! location recorded, a case that does not finish within the budget is a hang. A stack overflow aborts the process;
//! that is also a finding, and the case that caused it is the last one printed with `LAYOUTCRAFT_HOSTILE_TRACE=1`.
//!
//! Environment: `LAYOUTCRAFT_HOSTILE_SCALE` multiplies the number of random cases (default 1, which takes a few
//! seconds); `LAYOUTCRAFT_HOSTILE_SEED` changes the seed; `LAYOUTCRAFT_HOSTILE_BUDGET_MS` the per-case time budget;
//! `LAYOUTCRAFT_HOSTILE_DUMP=<dir>` writes the input of every distinct failure to that directory.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::io::Write;
use std::sync::mpsc;
use std::sync::Once;
use std::time::{Duration, Instant};

/// SplitMix64: small, fixed, the same on every platform.
pub struct Rng(pub u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// A number below `n` (`n` > 0).
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    pub fn chance(&mut self, one_in: usize) -> bool {
        self.below(one_in) == 0
    }
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

pub fn seed() -> u64 {
    std::env::var("LAYOUTCRAFT_HOSTILE_SEED").ok().and_then(|v| v.parse().ok()).unwrap_or(0x4C41_594F_5554)
}

/// How many times the default number of random cases to run.
pub fn scale() -> usize {
    std::env::var("LAYOUTCRAFT_HOSTILE_SCALE").ok().and_then(|v| v.parse().ok()).filter(|n: &usize| *n >= 1).unwrap_or(1)
}

fn budget() -> Duration {
    Duration::from_millis(std::env::var("LAYOUTCRAFT_HOSTILE_BUDGET_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(20_000))
}

// ---- the guarded runner ----------------------------------------------------------------------------------------------------------------

thread_local! {
    static IN_CASE: Cell<bool> = const { Cell::new(false) };
    static WHERE: RefCell<String> = const { RefCell::new(String::new()) };
}

static HOOK: Once = Once::new();

fn install_hook() {
    HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if IN_CASE.with(|flag| flag.get()) {
                let place = info.location().map(|l| format!("{}:{}", l.file().replace('\\', "/"), l.line())).unwrap_or_default();
                let trace = if std::env::var_os("LAYOUTCRAFT_HOSTILE_BACKTRACE").is_some() { format!("
{}", std::backtrace::Backtrace::force_capture()) } else { String::new() };
                WHERE.with(|w| *w.borrow_mut() = format!("{place}{trace}"));
            } else {
                previous(info);
            }
        }));
    });
}

pub struct Failure {
    pub signature: String,
    pub first_case: String,
    pub count: usize,
    pub input: Vec<u8>,
}

/// Collects the distinct failures (a panic message and place, or a hang) of one sweep.
pub struct Sweep {
    pub name: &'static str,
    pub cases: usize,
    pub failures: BTreeMap<String, Failure>,
    started: Instant,
    hangs: usize,
    /// The slowest cases: (seconds, label).
    slow: Vec<(f64, String)>,
}

impl Sweep {
    pub fn new(name: &'static str) -> Self {
        install_hook();
        Sweep { name, cases: 0, failures: BTreeMap::new(), started: Instant::now(), hangs: 0, slow: Vec::new() }
    }

    /// Run `f` on `input` as one case. Returns false when it failed.
    pub fn run<F>(&mut self, label: &str, input: Vec<u8>, f: F) -> bool
    where
        F: FnOnce(&[u8]) + Send + 'static,
    {
        if self.hangs >= 3 {
            return true; // threads of hung cases still run: do not pile up more of them
        }
        self.cases += 1;
        if std::env::var_os("LAYOUTCRAFT_HOSTILE_TRACE").is_some() {
            eprintln!("[{}] {} ({} bytes)", self.name, label, input.len());
        }
        let (tx, rx) = mpsc::channel();
        let copy = input.clone();
        let began = Instant::now();
        let spawned = std::thread::Builder::new().name(self.name.to_string()).stack_size(2 * 1024 * 1024).spawn(move || {
            IN_CASE.with(|flag| flag.set(true));
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&copy)));
            let outcome = match result {
                Ok(()) => None,
                Err(payload) => {
                    let message = payload
                        .downcast_ref::<&str>()
                        .map(|s| s.to_string())
                        .or_else(|| payload.downcast_ref::<String>().cloned())
                        .unwrap_or_else(|| "panic".to_string());
                    let message: String = message.chars().take(120).collect();
                    Some(format!("panic at {}: {}", WHERE.with(|w| w.borrow().clone()), message))
                }
            };
            let _ = tx.send(outcome);
        });
        if spawned.is_err() {
            return true;
        }
        let answer = rx.recv_timeout(budget());
        let took = began.elapsed().as_secs_f64();
        if took > 1.0 {
            self.slow.push((took, label.to_string()));
        }
        match answer {
            Ok(None) => true,
            Ok(Some(signature)) => {
                self.record(signature, label, input);
                false
            }
            Err(_) => {
                self.hangs += 1;
                self.record(format!("no answer within {:?} (a hang)", budget()), label, input);
                false
            }
        }
    }

    fn record(&mut self, signature: String, label: &str, input: Vec<u8>) {
        let entry = self.failures.entry(signature.clone()).or_insert_with(|| Failure { signature, first_case: label.to_string(), count: 0, input });
        entry.count += 1;
    }

    /// Fail the test when any case failed, listing each distinct failure once.
    pub fn finish(self) {
        let elapsed = self.started.elapsed();
        eprintln!("[{}] {} cases, {} distinct failures, {:.1?}", self.name, self.cases, self.failures.len(), elapsed);
        for (seconds, label) in self.slow.iter().take(5) {
            eprintln!("[{}]   slow case ({seconds:.1} s): {label}", self.name);
        }
        if self.failures.is_empty() {
            return;
        }
        let dump = std::env::var("LAYOUTCRAFT_HOSTILE_DUMP").ok();
        let mut text = String::new();
        for (n, failure) in self.failures.values().enumerate() {
            text.push_str(&format!("  #{n} x{}: {}\n      first case: {}\n", failure.count, failure.signature, failure.first_case));
            if let Some(dir) = &dump {
                let path = format!("{dir}/{}-{n}.bin", self.name);
                let _ = std::fs::write(&path, &failure.input);
                text.push_str(&format!("      input written to {path}\n"));
            }
        }
        panic!("{}: {} distinct failures in {} cases\n{text}", self.name, self.failures.len(), self.cases);
    }
}

// ---- byte mutators -----------------------------------------------------------------------------------------------------------------------

const INTERESTING_U32: [u32; 12] = [0, 1, 0x7F, 0xFF, 0xFFFF, 0x7FFF_FFFF, 0x8000_0000, 0xFFFF_FFFF, 0xFFFF_FFFE, 0x0100_0000, 0x00FF_FFFF, 0xDEAD_BEEF];

/// One random edit of the bytes: flips, a byte run, a 4-byte "interesting" number, a cut, a deleted or a repeated range.
pub fn mutate_bytes(rng: &mut Rng, bytes: &mut Vec<u8>) {
    if bytes.is_empty() {
        bytes.push(rng.next() as u8);
        return;
    }
    match rng.below(8) {
        0 | 1 => {
            for _ in 0..1 + rng.below(8) {
                let at = rng.below(bytes.len());
                bytes[at] ^= 1 << rng.below(8);
            }
        }
        2 => {
            let at = rng.below(bytes.len());
            let run = 1 + rng.below(16);
            let value = *rng.pick(&[0u8, 0xFF, 0x7F, 0x80, b'<', b'"', b' ']);
            for b in bytes.iter_mut().skip(at).take(run) {
                *b = value;
            }
        }
        3 => {
            if bytes.len() >= 4 {
                let at = rng.below(bytes.len() - 3);
                let v = *rng.pick(&INTERESTING_U32);
                bytes[at..at + 4].copy_from_slice(&v.to_le_bytes());
            }
        }
        4 => {
            let keep = rng.below(bytes.len());
            bytes.truncate(keep);
        }
        5 => {
            let at = rng.below(bytes.len());
            let len = rng.below(64).min(bytes.len() - at);
            bytes.drain(at..at + len);
        }
        6 => {
            let at = rng.below(bytes.len());
            let len = (1 + rng.below(64)).min(bytes.len() - at);
            let chunk: Vec<u8> = bytes[at..at + len].to_vec();
            let to = rng.below(bytes.len());
            bytes.splice(to..to, chunk);
        }
        _ => {
            let at = rng.below(bytes.len());
            bytes.insert(at, rng.next() as u8);
        }
    }
}

/// Values a number or a name attribute may hold in a hostile file.
pub const HOSTILE_VALUES: [&str; 22] = [
    "NaN", "inf", "-inf", "Infinity", "1e308", "-1e308", "1e-320", "-0", "0", "-1", "", " ", "99999999999999999999", "4294967296", "2147483648",
    "-2147483649", "18446744073709551616", "0.0000000001", "1e400", "#", "THEMEVAL(", "=SUM(",
];

/// Replace the value of one random `="..."` attribute of `text` by a hostile value (or by a copy of another value: an id collision).
pub fn mutate_attribute(rng: &mut Rng, text: &str) -> String {
    let spots: Vec<usize> = text.match_indices("=\"").map(|(i, _)| i + 2).collect();
    if spots.is_empty() {
        return text.to_string();
    }
    let at = *rng.pick(&spots);
    let Some(len) = text[at..].find('"') else { return text.to_string() };
    let replacement = if rng.chance(4) {
        let other = *rng.pick(&spots);
        text[other..].find('"').map(|n| text[other..other + n].to_string()).unwrap_or_default()
    } else {
        rng.pick(&HOSTILE_VALUES).to_string()
    };
    format!("{}{}{}", &text[..at], replacement, &text[at + len..])
}

/// Mutate XML text at the text level: a hostile attribute value, a cut in the middle of an element, a deleted or repeated element, a wrong closing tag.
pub fn mutate_xml(rng: &mut Rng, text: &str) -> String {
    match rng.below(7) {
        0..=2 => mutate_attribute(rng, text),
        3 => {
            let tags: Vec<usize> = text.match_indices('<').map(|(i, _)| i).collect();
            if tags.is_empty() {
                return String::new();
            }
            let at = *rng.pick(&tags);
            let mut end = (at + 1 + rng.below(12)).min(text.len());
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text[..end].to_string()
        }
        4 => {
            // delete one tag (from a '<' to the next '>')
            let tags: Vec<usize> = text.match_indices('<').map(|(i, _)| i).collect();
            if tags.is_empty() {
                return text.to_string();
            }
            let at = *rng.pick(&tags);
            let Some(len) = text[at..].find('>') else { return text.to_string() };
            format!("{}{}", &text[..at], &text[at + len + 1..])
        }
        5 => {
            // repeat a stretch of markup: duplicate ids, deeper nesting
            let tags: Vec<usize> = text.match_indices('<').map(|(i, _)| i).collect();
            if tags.len() < 2 {
                return text.to_string();
            }
            let a = *rng.pick(&tags);
            let b = *rng.pick(&tags);
            let (a, b) = (a.min(b), a.max(b));
            let mut stop = b.min(a + 600);
            while !text.is_char_boundary(stop) {
                stop -= 1;
            }
            let stretch = &text[a..stop];
            let times = 1 + rng.below(3);
            format!("{}{}{}", &text[..a], stretch.repeat(times + 1), &text[stop..])
        }
        _ => {
            let tags: Vec<usize> = text.match_indices("</").map(|(i, _)| i).collect();
            if tags.is_empty() {
                return text.to_string();
            }
            let at = *rng.pick(&tags);
            format!("{}</X{}", &text[..at], &text[at + 2..])
        }
    }
}

/// Mutate JSON text: replace a number with a hostile value, cut it, or change bytes.
pub fn mutate_json(rng: &mut Rng, text: &str) -> String {
    match rng.below(4) {
        0 | 1 => {
            let starts: Vec<usize> = text.char_indices().filter(|(i, c)| c.is_ascii_digit() && (*i == 0 || !text.as_bytes()[*i - 1].is_ascii_alphanumeric())).map(|(i, _)| i).collect();
            if starts.is_empty() {
                return text.to_string();
            }
            let at = *rng.pick(&starts);
            let end = text[at..].find(|c: char| !(c.is_ascii_digit() || c == '.' || c == 'e' || c == '-')).map(|n| at + n).unwrap_or(text.len());
            let value = *rng.pick(&["1e308", "-1e308", "1e999", "-0", "0", "99999999999999999999", "4294967296", "-1", "1e-320", "0.5", "2147483648", "9007199254740993", "null", "\"x\"", "[]", "{}", "true"]);
            format!("{}{}{}", &text[..at], value, &text[end..])
        }
        2 => {
            let mut end = rng.below(text.len().max(1));
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text[..end].to_string()
        }
        _ => {
            let mut bytes = text.as_bytes().to_vec();
            mutate_bytes(rng, &mut bytes);
            String::from_utf8_lossy(&bytes).into_owned()
        }
    }
}

// ---- a ZIP writer that can lie --------------------------------------------------------------------------------------------------------

pub fn crc32(data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, slot) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *slot = c;
        }
        t
    });
    let mut c = 0xFFFF_FFFFu32;
    for b in data {
        c = table[((c ^ *b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

pub fn deflate(data: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
    let _ = encoder.write_all(data);
    encoder.finish().unwrap_or_default()
}

/// One entry of a ZIP file to write. `size` is what the headers say the uncompressed size is.
#[derive(Clone)]
pub struct Entry {
    pub name: String,
    /// The data as it is stored: raw bytes for method 0, a raw deflate stream for method 8.
    pub stored: Vec<u8>,
    pub method: u16,
    pub size: u32,
    pub crc: u32,
}

/// An entry with real data, deflated or stored, with the true sizes in the headers.
pub fn honest(name: &str, data: &[u8], deflated: bool) -> Entry {
    let stored = if deflated { deflate(data) } else { data.to_vec() };
    Entry { name: name.to_string(), stored, method: if deflated { 8 } else { 0 }, size: data.len() as u32, crc: crc32(data) }
}

pub fn write_zip(entries: &[Entry]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for entry in entries {
        let offset = out.len() as u32;
        let csize = entry.stored.len() as u32;
        let name = entry.name.as_bytes();
        out.extend(0x0403_4b50u32.to_le_bytes());
        out.extend(20u16.to_le_bytes());
        out.extend(0u16.to_le_bytes());
        out.extend(entry.method.to_le_bytes());
        out.extend(0u16.to_le_bytes());
        out.extend(0x21u16.to_le_bytes());
        out.extend(entry.crc.to_le_bytes());
        out.extend(csize.to_le_bytes());
        out.extend(entry.size.to_le_bytes());
        out.extend((name.len() as u16).to_le_bytes());
        out.extend(0u16.to_le_bytes());
        out.extend(name);
        out.extend(&entry.stored);
        central.extend(0x0201_4b50u32.to_le_bytes());
        central.extend(20u16.to_le_bytes());
        central.extend(20u16.to_le_bytes());
        central.extend(0u16.to_le_bytes());
        central.extend(entry.method.to_le_bytes());
        central.extend(0u16.to_le_bytes());
        central.extend(0x21u16.to_le_bytes());
        central.extend(entry.crc.to_le_bytes());
        central.extend(csize.to_le_bytes());
        central.extend(entry.size.to_le_bytes());
        central.extend((name.len() as u16).to_le_bytes());
        central.extend([0u8; 8]); // extra length, comment length, disk number, internal attributes
        central.extend(0u32.to_le_bytes()); // external attributes
        central.extend(offset.to_le_bytes());
        central.extend(name);
    }
    let cd_offset = out.len() as u32;
    out.extend(&central);
    out.extend(0x0605_4b50u32.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out.extend((entries.len().min(0xFFFF) as u16).to_le_bytes());
    out.extend((entries.len().min(0xFFFF) as u16).to_le_bytes());
    out.extend((central.len() as u32).to_le_bytes());
    out.extend(cd_offset.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out
}

/// A zip made of `parts` (all honest), each deflated or stored by a coin flip of `rng`.
pub fn zip_of(rng: &mut Rng, parts: &[(String, Vec<u8>)]) -> Vec<u8> {
    let entries: Vec<Entry> = parts.iter().map(|(name, data)| honest(name, data, rng.chance(2))).collect();
    write_zip(&entries)
}
