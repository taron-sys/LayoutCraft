# 47 Old Visio formats (`.vsd`, `.vdx` and their stencil and template forms)

`layoutcraft_vsdx::legacy` reads a drawing in an old format and writes a **new `.vsdx` package**. The engine then treats it like any
other drawing, so rendering, editing, undo, export and saving need no second code path. The old file is never written.

| Format | Extensions | State |
|---|---|---|
| Binary, file version 11 (Visio 2003-2010) | `.vsd` `.vss` `.vst` | read |
| Binary, file version 6 (Visio 2000-2002) | `.vsd` `.vss` `.vst` | read |
| Binary, file version 5 and older | `.vsd` `.vss` `.vst` | refused with a message that names the version |
| Visio 2003 XML | `.vdx` `.vsx` `.vtx` | read |

The format is recognised by content, not by name. `Engine::open` and `Engine::stencil_open` convert, so the desktop app and the MCP
server get the same result; `Engine::conversion` holds the message the host shows. The web shell does not offer these extensions in its
file pickers yet.

## What is carried over

Pages (with background pages and layers), masters and master inheritance, style sheets, shape transforms, geometry (lines, arcs,
elliptical arcs, ellipses, splines, polylines, NURBS, infinite lines), connection points, line, fill and shadow formatting, text with
character and paragraph runs and text block settings, shape data, colour table, fonts and pictures (bitmaps and metafiles). From XML
drawings also the formulas, the connections and the other sections the file holds.

## What is not

* Binary: **formulas** (only literal strings and curve point lists are read), so edited shapes scale plainly; **glue** between
  connectors and shapes; user cells, hyperlinks, controls, actions, guides' behaviour; document properties.
* Binary: **text fields** become the value saved in the file, formatted here as general or 0-3 decimal numbers, with units, or as
  `yyyy-mm-dd` dates. Other format pictures are approximated.
* Both: macros, embedded OLE objects, links to external data. The conversion notes name what was found and dropped.
* Binary text before version 11 is read as Windows-1252; other code pages show wrong letters.

## XML drawings without a namespace

Some programs write the XML format with no namespace at all; one sample, which marks itself as exported by Lucidchart, does. Such a
file is recognised when its first element is a `VisioDocument` with content and no default namespace. Its cells are read as written:
in that sample, the parts of a few groups are placed on top of each other by the file's own coordinates. How Visio shows that file
has not been checked.

## Evidence and provenance

Container: the public specification [MS-CFB]. XML format: the published Visio 2003 XML schema reference and drawings saved both as
`.vdx` and `.vsdx`.

Microsoft publishes no specification of the binary drawing format. The starting hypothesis for its layout (stream pointers, the
compression, record headers and field positions) came from the general knowledge of the AI model that wrote the reader, which includes
public descriptions of the format and of open-source readers (Apache POI, Apache-2.0; libvisio, MPL-2.0). **No source file of a
third-party reader was opened or copied during the work.** The positions used were then checked against sample files:

* the record tree was walked and decoded on the samples under `examples/Documents/` (third-party test files, git-ignored, not part of
  this repository);
* rendered pages were compared with LibreOffice's rendering of the same files (used as a black box);
* page cells of a drawing saved as `.vsd` and `.vsdx` agree, and both render the same;
* measured on 2,044 instance shapes: the row list of an instance's geometry section names exactly the rows that have records (5,297
  sections), and rows only the master has are never listed. The reader therefore marks those rows deleted in sections that begin with
  a start row.

Not verified against Visio itself (AGENTS.md rule 26): no COM probe was run. Instance geometry sections are paired with the master's by
position; where the files' section ids differ from the master's, position agreed with the row types in about 97 % of rows.

## Layout of the binary format, as used

Compound file stream `VisioDocument`: the text `Visio (TM) Drawing`, the file version at `0x1a`, and at `0x24` the pointer to the root
stream. A pointer is 18 bytes: type u32, 4 bytes, offset u32, length u32, format u16. Format bit 2 means compressed (an LZ77 variant:
flag byte, literals or 12-bit position and 4-bit length into a 4096-byte window). A high format nibble of `5` marks a list of pointers
(at the offset the stream starts with: order count, pointer count, 4 bytes, pointers, order); `8`, `c`, `d` mark records; anything
else is one blob.

A record has a 19-byte header (type u32, id u32, list u32, data length u32, level u16, 1 byte), its data, a trailer and zero padding.
A "cell" in the data is a unit byte and an f64 in internal units. The record types and field offsets are the constants and functions of
`rust/layoutcraft-vsdx/src/legacy/vsd/build.rs`, which is the reference for them.

## Measured on the samples (2026-11-26)

240 files with an old extension under `examples/Documents/`: 233 converted and imported. The 7 others: five of file version 5 (two of
them damaged fuzzer files), one damaged compound file, one fuzzer file that is neither format.

## Tests

`rust/layoutcraft-vsdx/tests/it/legacy.rs` builds a small binary drawing and a small XML drawing by hand (our own work) and also runs a
mutation sweep over both. There is no test that depends on the third-party samples.
