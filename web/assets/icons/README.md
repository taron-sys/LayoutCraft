# LayoutCraft icons

One SVG per command, 24 x 24, drawn with strokes (1.6 wide, round ends and joins). Edit the files directly (any text or SVG editor): the application reads them as they are.

**Provenance:** every icon is drawn by this project from hand-written stroke paths. No third-party or Microsoft icon artwork is copied
([`PROVENANCE.md`](../../../PROVENANCE.md)).

How the application uses them: each ribbon button shows its icon through a CSS **mask** (`web/ribbon.js` lists which icon belongs to which button, `web/assets/icons/index.html` shows all of them). A mask only looks at the
**shape** of the drawing (where it is opaque), not at its colours: the icon is painted in the text colour of the current palette (light, dark, ...). So:

* draw in ONE colour (black is fine); colours you use are ignored;
* keep the 24 x 24 viewBox and stay about 2 units away from the edge;
* a part you want half-transparent: `fill-opacity=".25"`; a filled dot: `fill="#000" stroke="none"`;
* no white "holes" painted on top (a mask would show them as solid): leave the gap in the line work, or use a path with fill-rule="evenodd";
* to add an icon for a new button: save `name.svg` here and add `'buttonId': 'name'` to the ICONS table in `web/ribbon.js`.

Open `index.html` in a browser (a double click is enough) to see the whole set at 24 px and 48 px on a light and a dark background, with its file names.
