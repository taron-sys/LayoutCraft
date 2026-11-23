# Fonts the vector PDF embeds

Put open-licence TrueType files (`.ttf`) here and name them in `manifest.json`. When a drawing uses a font family listed under `aliases`, the PDF export
embeds a SUBSET of the matching file (only the glyphs the drawing uses); every other family keeps the 14 standard PDF fonts. A file that is missing, is not
TrueType (OpenType with CFF outlines, `.otf` / `.ttc` are refused) or carries the "restricted licence" embedding bit is skipped without an error.

The manifest asks for these files (all under open licences, metric compatible with the common Windows fonts):

| Files | Project | Licence | Stands in for |
|---|---|---|---|
| `LiberationSans-*.ttf`, `LiberationSerif-*.ttf`, `LiberationMono-*.ttf` | Liberation Fonts 2.x | SIL OFL 1.1 | Arial / Helvetica, Times New Roman, Courier New |
| `Carlito-*.ttf` | Carlito | SIL OFL 1.1 | Calibri |
| `Caladea-*.ttf` | Caladea | SIL OFL 1.1 (the google/fonts copy; the Chrome OS extra-fonts release says Apache 2.0, read the licence file) | Cambria |

Each family has up to four files (`Regular`, `Bold`, `Italic`, `BoldItalic`); a missing style falls back to the regular one. Keep the licence texts next to
the files. Do not put proprietary fonts (Arial, Calibri, Segoe UI ...) here: their licences do not allow redistribution.


Installed 2026-11-16 (step 254, user answer Q1): Liberation Fonts 2.1.5 (github.com/liberationfonts/liberation-fonts release 2.1.5), Carlito and Caladea (github.com/google/fonts, ofl/carlito and ofl/caladea). Licence texts: LICENSE-Liberation.txt, LICENSE-Carlito-OFL.txt, LICENSE-Caladea-OFL.txt.
