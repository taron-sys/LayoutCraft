# 43 UI parity with Visio

> **Status:** planning document, written 2026-11-25. Nothing here is implemented by this change. All hour figures are **estimates**
> (one developer, including a manual check in the app), not measurements.

LayoutCraft opens and saves `.vsdx` files. People who know Visio should find the same layout, the same order of things and the same
keys when they open LayoutCraft. This document compares the two user interfaces and ranks the work needed to follow Visio's layout
and conventions. It follows layout, arrangement, grouping, order and behaviour. It does not follow artwork: every icon, colour and
font in LayoutCraft is drawn or chosen by us (`AGENTS.md` rules 16 and 19).

This is an internal comparison document. Microsoft command names appear in it as references so that a reader can find the feature.
Names shown to users in LayoutCraft follow the proposals in section 6, each marked "needs naming check".

## 1. Scope and sources

### 1.1 Scope

- In: the desktop app (`rust/layoutcraft-app`, egui) and the browser shell (`web/`, being retired), both driven by the one ribbon
  definition `rust/layoutcraft-app/src/ribbon-web.json`.
- Out: file-format behaviour (see `docs/14-v1-scope-provenance.md`), stencil content, templates, and any look-alike artwork.
- Visio editions: Visio on Windows (desktop) is the reference. Visio for the web is mentioned where a source describes it.

### 1.2 How this was written

The Visio side of every row comes from the public pages listed below, read through a web fetch tool on 2026-11-25. Visio was not
run and nothing from an installation was read. Where a fact is not in a cited page it is marked **unverified**. Help text is
summarised in our own words. The LayoutCraft side comes from reading the code at `origin/main` (commands in
`rust/layoutcraft-app/src/ribbon.rs` `native()`, the layout in `rust/layoutcraft-app/src/main.rs`, the browser shell in
`web/shell.js`, `web/ribbon.js`, `web/keyboard.js`, `web/contextmenu.js`). This statement describes how this document was produced;
it makes no claim about how the rest of the project was built (`PROVENANCE.md`).

### 1.3 Sources (all accessed 2026-11-25)

Pages marked "(search snippet)" were seen only as a search-result summary, not opened in full; treat the facts taken from them as
weaker.

| Id | Page | URL |
|---|---|---|
| S1 | Keyboard shortcuts for Visio (desktop and web tables) | https://support.microsoft.com/en-US/accessibility/visio/keyboard-shortcuts-for-visio |
| S2 | Use the Shapes window to organize and find shapes | https://support.microsoft.com/en-US/Visio/use-the-shapes-window-to-organize-and-find-shapes |
| S3 | Find most-used shapes easily with Quick Shapes | https://support.microsoft.com/en-et/Visio/find-most-used-shapes-easily-with-quick-shapes-1 |
| S4 | The Office Fluent UI in the Visio 2010 technical preview (tab overview, status bar, Quick Access Toolbar; 2009 blog, old) | https://techcommunity.microsoft.com/blog/microsoft_365blog/the-office-fluent-ui-in-the-visio-2010-technical-preview/237338 |
| S5 | Beginner tutorial for Visio | https://support.microsoft.com/en-us/visio/beginner-tutorial-for-visio |
| S6 | Add and connect shapes in Visio | https://support.microsoft.com/en-gb/office/add-and-connect-shapes-in-visio-2b9cb32f-98fe-49ed-bd6a-d763e70e667c |
| S7 | Add connectors between Visio shapes (search snippet) | https://support.microsoft.com/en-us/visio/add-connectors-between-visio-shapes |
| S8 | Format in Visio | https://support.microsoft.com/en-gb/office/format-in-visio-15412a4f-c59a-4fe0-8dd4-d466e6250137 |
| S9 | Format a shape in Visio | https://support.microsoft.com/en-us/visio/format-a-shape-in-visio |
| S10 | Improve shape placement by using the dynamic grid | https://support.microsoft.com/en-us/topic/53feab39-341f-4d4f-bfaa-af59bbe515b5 |
| S11 | Zoom and pan in a diagram | https://support.microsoft.com/en-US/Visio/zoom-and-pan-in-a-diagram |
| S12 | Set the dimensions and position of a shape by using the Size & Position window | https://support.microsoft.com/en-US/Visio/set-the-dimensions-and-position-of-a-shape-by-using-the-size-position-window |
| S13 | Align and position shapes in a diagram | https://support.microsoft.com/en-au/visio/align-and-position-shapes-in-a-diagram |
| S14 | The Developer tab in Visio 2010 (2009 blog, old) | https://techcommunity.microsoft.com/blog/microsoft_365blog/the-developer-tab-in-visio-2010/237385 |
| S15 | Review and add comments to Visio diagrams | https://support.microsoft.com/en-us/visio/review-and-add-comments-to-visio-diagrams |
| S16 | Customizing the Visio UI (third-party tutorial: Quick Access Toolbar, Customize Ribbon) | https://www.tutorialspoint.com/microsoft_visio/microsoft_visio_customizing_ui.htm |
| S17 | Visio interface and layout (third-party book chapter, Visio 2003/2007 menus; used only for the Shapes window, rulers, grid, status bar) | https://leanpub.com/read/network-diagrams-with-visio/leanpub-auto-visio-interface-and-layout |
| S18 | Trim lines and shapes in Visio drawings | https://support.microsoft.com/en-us/visio/trim-lines-and-shapes-in-visio-drawings |
| S19 | Select, move, resize, and rotate shapes | https://support.microsoft.com/en-us/visio/select-move-resize-and-rotate-shapes |
| S20 | Clarify the structure of diagrams by using containers and lists | https://support.microsoft.com/en-us/visio/clarify-the-structure-of-diagrams-by-using-containers-and-lists |
| S21 | Show, hide, or change the grid in Visio | https://support.microsoft.com/en-us/visio/show-hide-or-change-the-grid-in-visio |
| S22 | Use layers to set properties for multiple shapes; Add a layer (search snippet) | https://support.microsoft.com/en-us/visio/use-layers-to-set-properties-for-multiple-shapes , https://support.microsoft.com/en-us/visio/add-a-layer |
| S23 | Re-layout a diagram in Visio (search snippet); Shape.Layout method | https://support.microsoft.com/bg-BG/Visio/re-layout-a-diagram-in-visio , https://learn.microsoft.com/en-us/office/vba/api/visio.shape.layout |
| S24 | Automatically link imported data to shapes; Add a legend to data graphics (search snippets) | https://support.microsoft.com/en-us/visio/automatically-link-imported-data-to-shapes , https://support.microsoft.com/en-us/visio/add-a-legend-to-data-graphics |
| S25 | Annotate a diagram by using callouts (search snippet); Inserting containers and callouts, third-party (search snippet, lists the Insert tab groups) | https://support.microsoft.com/en-us/visio/annotate-a-diagram-by-using-callouts , https://tutorialspoint.com/microsoft_visio/microsoft_visio_inserting_containers_callouts.htm |
| S26 | Show or hide rulers in Visio (search snippet) | https://support.microsoft.com/en-us/visio/show-or-hide-rulers-in-visio |
| S27 | What's new in Visio Professional 2021 and Visio 2024 (search snippets: refreshed ribbon tabs, new search bar) | https://support.microsoft.com/office/what-s-new-in-visio-professional-2021-96732c1e-a022-4362-af07-fc817011ced5 , https://support.microsoft.com/en-US/Visio/what-s-new-in-visio-2024 |
| S28 | Auto-align and space the shapes in a diagram (search snippet) | https://support.microsoft.com/en-us/visio/auto-align-and-space-the-shapes-in-a-diagram |

### 1.4 Gaps in the sources

- No fetched page lists the **complete** command set of any tab. The Home, Design, View and Developer rows below combine the pages
  above; commands that only the LayoutCraft ribbon lists are marked "unverified (no cited page)".
- The **Draw** tab, **Help** tab, **Tell me** search box, current **status bar** contents, **Backstage** page list, **Format Shape**
  pane section list beyond Fill & Line and Effects, **mini toolbar on selection** (as opposed to the connect arrows' mini toolbar), the
  **Process** tab's current commands and the **Window** group of the View tab were not found in a page I could open: unverified.
- Alt key tips: the shortcut page (S1) gives only Alt+H for Home. Key-tip letters for the other tabs are unverified from public
  sources (see 4.6 for a repository note).
- S4, S14 describe Visio 2010. Current Visio has since refreshed its tabs (S27) so group contents may differ.

## 2. Window layout

Priority: P0 a Visio user would not feel at home without it; P1 expected within the first hour; P2 expected by regular users; P3
nice to have. Effort is an estimate in hours.

| Area | Visio (source) | LayoutCraft today | Gap | Prio | Est. h |
|---|---|---|---|---|---|
| Title bar and Quick Access Toolbar | Upper-left toolbar with Undo, Redo, Save by default, extendable via a drop-down and More Commands (S16, S4). | Desktop draws a save / undo / redo strip above the tabs (`draw_ribbon`); browser has `web/qat.js`. | No user customisation on desktop (add, remove, reorder). Browser customisation unverified in this pass. | P2 | 8-12 |
| Ribbon tabs | Home, Insert, Design, Data, Process, Review, View, Developer (hidden until enabled), Help; the page tab order in S4 lists Home through View; Draw tab unverified (S4, S14, S27). | `ribbon-web.json`: File, Home, Insert, Draw, Design, Data, Process, Review, View, Help. Draw and Process carry a "not built" note. No Developer tab. | Developer tab missing; Draw and Process are stubs; tab can be collapsed (Ctrl+F1 on desktop). | P1 | 6-10 |
| Contextual tabs | A tab appears for a selected container (Container Format; S20) and for some templates (a Plan tab with the office-layout template; S5). Picture, connector and other contextual tabs: unverified. | None. Options for the selection live in the right-hand inspector (desktop) or panel (browser). | Contextual-tab mechanism absent in both hosts. | P2 | 16-24 |
| Ribbon layout | Groups with a launcher (dialog box) at the group corner (S10, S21 mention group launchers); ribbon can collapse and be customised through File > Options > Customize Ribbon (S16). | Groups lay out in two rows (`grid_cell`), compact mode below 1750 px, collapse with Ctrl+F1, tab drag not supported; no launchers; no customise. | Launchers; customise ribbon. | P2 | 10-20 |
| File / Backstage | File opens a full-page view; New shows a template gallery with categories and search (S5); Options holds Customize Ribbon and Advanced (S16, S11). | Browser: `web/backstage.js` (New, Open, Export, Info). Desktop: File opens a full page with the file commands at the side, a blank drawing, a path to open and the recent drawings. | No template gallery; no Options page. | P1 | 14-24 |
| Shapes window | Stencils stacked, one open at a time, title bars; More Shapes menu; Search at the top; minimise to a thin column; any stencil can be opened in any diagram (S2). Quick Shapes area at the top of each stencil plus a generated Quick Shapes stencil (S3). | Desktop left panel: heading, "Open stencil", "Basic shapes", search box, minimise to a narrow column, stencil headers with one stencil open at a time (a search shows all matches), 70 x 84 px thumbnails; click-then-click or drag onto page; context menu Place / Replace; remembers opened stencils. Browser: `web/shell.js` palette with favourites. | More Shapes category menu, Quick Shapes area and stencil, drag shape out of the Quick area to edit it, Shapes window dock/float. | P0 | 16-28 |
| Drawing canvas | Page with horizontal and vertical rulers (S26), optional grid (S21) fixed or variable, guides dragged from rulers (S26 snippet), page breaks (unverified), workspace beyond the page. | Desktop: page raster with grid lines (`show_grid`), snapping to grid / shapes / guides read from the page; rulers (no drawing scale), guides dragged from them. Browser: rulers, guides (`shell.js` ruler guides), grid, page breaks. | Drawing scale on the desktop rulers; keyboard creation and moving of guides; page-break view. | P0 | 12-20 |
| Page tabs | Tabs at the lower edge of the drawing: Page-1 and background pages such as VBackground-1 (S5); right-click a page tab for page properties (S1 snippet context, S27). | Browser: `#pageBar` with context menu (`contextmenu.js`). Desktop: a tab strip under the drawing with a right-click menu, drag to reorder and an add button; page buttons also in the status bar. | Background pages in the desktop strip; page properties from the menu; keyboard reordering other than the ribbon commands. | P0 | 8-14 |
| Status bar | Left: items that act as shortcuts, e.g. clicking the width item opens Size & Position (S4); right: zoom controls, Full Screen, window switching (S4); zoom slider and Fit Page to Window at the lower right, and a button to open Pan & Zoom (S11). Page number and language: unverified. | Desktop bottom panel: previous / next page, "page n / m", status message, zoom slider, "zoom nn%", Fit and Full Screen buttons, "modified", and the selected shape's width, height and angle (a click goes to Size and Position). Browser: `#statusbar`. | Pan button; window switching; language item unverified. | P0 | 8-12 |
| Zoom and pan | Zoom slider, Fit Page to Window, Pan & Zoom button (S11); Ctrl+wheel zooms, plain wheel scrolls, Ctrl+Shift magnifier (click zoom in, right-click out, drag a rectangle) (S11). | Desktop: Ctrl+wheel zooms at the pointer, the wheel scrolls (Shift: sideways), middle button / Space / Pan tool move; Ctrl+= / Ctrl+-; zoom slider and menu; Ctrl+Shift magnifier (click zooms in, right click out, a dragged box fills the view, right drag pans); a Mouse wheel zooms option in the Zoom menu. Browser: wheel option (`wheelZoom`), Pan & Zoom pane. | Pan & Zoom pane. | P1 | 8-14 |
| Task pane: Size & Position | View > Show > Task Panes > Size & Position: Width, Height, X, Y (pin position), Pin Pos, Angle; units typed in text; formulas (S12). | Desktop inspector: editable pin X and Y, width, height and angle (shapes of the page, not locked), in the page unit (in or mm). Browser: `paneSizePos`. | Choice of the reference point; typed units; formulas (P3); a dockable window. | P0 | 8-14 |
| Task pane: Shape Data | Data entered per shape and shown in a window (View > Task Panes, Data > Show/Hide; S5). | Browser: Data panel (`dataBtn`). Desktop: a window for one selected shape (values, add, remove); a field's type (text, yes or no, list) and prompt can be set and changed, and a field hidden or shown. | Number and date types; renaming a field; order editing; several shapes at once; docking. | P1 | 10-16 |
| Task pane: Pan & Zoom | Thumbnail of the page with a blue frame; drag the frame to pan, resize to zoom, has a slider (S11). | Browser `panePanZoom` (frame drag, wheel). Desktop none. | Port to desktop. | P2 | 6-10 |
| Task pane: Layers | Layer Properties dialog with Visible, Print, Active, Lock, Snap, Glue, Color columns; Assign to Layer (S22). | Desktop inspector: per layer Visible / Prints / Locked checkboxes, shape-on-layer checkbox, add and delete layer. Browser: layers panel. | Active, Snap, Glue, Color columns; Assign to Layer command in the Editing group. | P2 | 8-12 |
| Task pane: Format Shape | Opened from the right-click menu or F3; tabs Fill & Line and Effects (Shadow, Reflection, Glow, 3-D Rotation) (S9, S1). | Desktop inspector "Style": fill, line colour, weight, pattern. Browser: Home > Shape Styles drop-down (fill, line, shadow). | Docked pane with tabs; text options; F3. | P1 | 14-22 |
| Task pane: ShapeSheet | Developer > Shape Design > Show ShapeSheet for a shape, page or document (S14). | Browser `paneSheet` (read-only table from `shape_sheet_json`). Desktop: a read-only Shape Formulas window for one shape. | See naming: we call it a formula table; read-only first. | P3 | 10-16 |
| Task pane behaviour | Panes can float or dock and be resized, opened from View > Show > Task Panes (S12, S16); F6 moves between panes (S1). | Desktop: two fixed side panels, resizable; no float; F6 not bound. Browser: F6 cycles regions (`keyboard.js`). | Dock / float; F6 on desktop. | P2 | 10-16 |
| Context (right-click) menus | Shape menu has special commands for the shape; Format Shape from it (S5, S9). Page-tab menu exists (S1, S27). Canvas menu: unverified. | Desktop: shape menu (Cut, Copy, Paste, Duplicate, Delete, Group, Ungroup, order, rotate, flip, Edit text, Style settings, Shape Fields) and empty-page menu (Paste, Page setup, Fit, snap toggles). Browser: `contextmenu.js`. | A Format Shape pane; Container submenu; Shift+F10 key (S1). | P1 | 6-10 |
| Mini toolbars | AutoConnect mini toolbar of up to four Quick Shapes over the arrow (S3, S5, S6). Selection mini toolbar for text: unverified. | Browser: AutoConnect-style arrows (`autoConnectToggle`). Desktop: connect arrows without a picker. | The picker at the arrows; selection mini toolbar. | P0 | 12-20 |

## 3. Ribbon parity

Desktop (2026-11-27): the ribbon leaves out the commands of the web ribbon that the app does not have, instead of showing them
greyed. Not built there: format painter, Arrange, Edit Points, Fit Curve, lock shape, insert picture, paper colour, print, fixed
page, page breaks, pan and zoom pane, a second "Compare File", the separate wheel zoom button (it is in the zoom menu), refit,
tip delay and new window. A row below that says "present" for the desktop only because the button was shown is out of date.

Reading the table:

- **Desktop** and **Browser** status are `present`, `partial` or `missing`. Desktop status comes from `native()` in `ribbon.rs` plus
  features drawn outside the ribbon in `main.rs` (a feature that exists only in a side panel is `partial`). Browser status means the
  command is listed in `ribbon-web.json` and its id is referenced by `web/shell.js`; I did not run each handler, so a `present`
  there means "wired in code", not "checked by hand".
- **LC id** is the command id in `ribbon-web.json` (a `p:` prefix is a property control). A dash means LayoutCraft has no command.
- A Visio command carries the source in the Src column; "unverified" means no cited page names it.
- Prio is the parity priority from section 2. Rows that LayoutCraft has but Visio does not (Compare, AI assistant) are in 3.14.

**Totals (rows of 3.1 to 3.13, counted by script from the tables):** 125 Visio commands listed. Desktop: 48 present, 26 partial,
51 missing. Browser: 79 present, 14 partial, 32 missing. A row can bundle several related commands (for example six drawing tools), so
these are row counts, not a count of every Visio button.

### 3.1 File / Backstage

| Visio command | Src | LC id | Desktop | Browser | Notes | Prio |
|---|---|---|---|---|---|---|
| New (template gallery, categories, search) | S5 | newBtn | partial | partial | New blank only; no gallery | P1 |
| Open | S1 | file | present | present | | P0 |
| Save | S16 | save | partial | present | Desktop Ctrl+S saves a copy (`save_copy`), not in place | P0 |
| Save As | unverified | save | present | present | | P1 |
| Export | unverified | exportBtn | present | present | | P2 |
| Copy as picture | unverified | copyImgBtn | partial | present | Desktop: the whole page; not a selection | P3 |
| Print | S1 (Ctrl+P is a web shortcut) | print | missing | present | Browser prints through the page | P1 |
| Info / document properties | unverified | docPropsBtn | present | present | | P2 |
| Options (Customize Ribbon, Advanced) | S16, S11 | - | missing | missing | | P2 |
| Close / recent files | unverified | - | partial | missing | Desktop: recent drawings on the File page; no Close | P2 |

### 3.2 Quick Access Toolbar

| Visio command | Src | LC id | Desktop | Browser | Notes | Prio |
|---|---|---|---|---|---|---|
| Undo | S16 | undo | present | present | | P0 |
| Redo | S16 | redo | present | present | | P0 |
| Save | S16 | save | partial | present | see 3.1 | P0 |
| Customise the toolbar (More Commands) | S16 | - | missing | partial | `qat.js` extent unverified | P2 |

### 3.3 Home

| Visio command | Src | LC id | Desktop | Browser | Notes | Prio |
|---|---|---|---|---|---|---|
| Clipboard: Paste | S4 (clipboard group named only in general) | paste | present | present | | P0 |
| Clipboard: Cut | S4 | cut | present | present | | P0 |
| Clipboard: Copy | S4 | copy | present | present | | P0 |
| Format Painter (Ctrl+Shift+P) | S1 | painter | missing | present | Engine copy format exists for browser | P1 |
| Font: family | S1 (Font group named) | p:fontFamily | present | present | Desktop: font list with search | P1 |
| Font: size and Increase/Decrease size (Ctrl+Shift+> and <) | S1 | p:fontSize | present | present | Desktop: smaller / larger buttons, a typed size, and Ctrl+Shift+> / < (US key positions) | P1 |
| Font: Bold / Italic / Underline | S1 | p:bold | present | present | Desktop: buttons and Ctrl+B / I / U | P1 |
| Font: strikethrough, caps, sub/superscript, font colour | S1 for caps and sub/superscript; others unverified | p:strike | partial | partial | Desktop: strikethrough, all capitals and text colour for the whole text; no sub / superscript or small caps | P2 |
| Paragraph: align left / centre / right / justify | S1 | p:alignL | present | present | Desktop: the whole text of the selected shapes | P1 |
| Paragraph: vertical align top / middle / bottom | S1 | p:alignL (menu) | missing | partial | | P2 |
| Paragraph: indent, line spacing | unverified | p:indLeft, p:lineSp | present | present | Desktop: fixed choices of left indent and line spacing, whole text | P3 |
| Tools: Pointer Tool (Ctrl+1) | S5, S1 | - (default tool) | present | partial | Desktop: the Select button in Home > Tools, and Ctrl+1 | P1 |
| Tools: Text (Ctrl+2) | S9, S1 | insertTextBox | present | present | | P1 |
| Tools: Text Block (Ctrl+Shift+4) | S5, S1 | - | missing | missing | Moves the text block inside a shape | P2 |
| Tools: Connector (Ctrl+3) | S6, S1 | connectTool | present | present | Drag shape to shape; glue to connection point or whole shape | P0 |
| Tools: Connection Point (Ctrl+Shift+1) | S1 | connPointTool | partial | present | Desktop: a click adds a point, a click on one chooses it and Delete removes it, a drag moves it; no placing on turned or flipped shapes | P1 |
| Tools: Rectangle (Ctrl+8), Ellipse (Ctrl+9), Line (Ctrl+6), Arc (Ctrl+7), Freeform (Ctrl+5), Pencil (Ctrl+4) | S1 | drawTools | partial | present | Desktop: rectangle, rounded rectangle, ellipse, diamond, triangle, line and text box; no arc, freeform or pencil | P0 |
| Tools: Crop (Ctrl+Shift+2) | S1 | - | missing | missing | Picture crop | P3 |
| Tools: Pan (hand) | none in S11 | handTool | present | present | LayoutCraft addition; Visio's pan is wheel, Ctrl+Shift drag, Pan & Zoom (S11) | P3 |
| Tools: Edit points / Fit curve | unverified | editPointsBtn, fitCurveBtn | missing | present | | P2 |
| Shape Styles: quick style gallery | S9 | p:fillColor (menu) | missing | partial | | P1 |
| Shape Styles: Fill (colour, no fill, more colours) | S9 | p:fillColor | present | present | Desktop: swatches, none and More colours (a typed code or a picker) on the ribbon, and the inspector's Style panel | P0 |
| Shape Styles: Line (colour, weight, dashes, arrows) | S9 | p:lineColor | present | present | Desktop: colour swatches and the Line ends menu (arrowheads and sizes) on the ribbon; weight and dash pattern in the inspector | P0 |
| Shape Styles: Effects (shadow, reflection, glow, soft edges, bevel, 3-D rotation) | S9 | p:shadow | partial | partial | Shadow only (on / off) | P2 |
| Arrange: Align Left / Center / Right / Top / Middle / Bottom | S13 | arrL arrCx arrRt arrT arrM arrB | present | present | Aligns to the first selected; Visio aligns to the primary shape (S13) | P0 |
| Arrange: Auto Align | S13 | - | missing | missing | Straightens connector lines | P3 |
| Arrange: Position > Auto Align & Space, Auto Space, Spacing Options | S13, S28 | - | missing | missing | | P2 |
| Arrange: Position > Distribute (three or more) | S13 | arrDistH arrDistV | present | present | | P0 |
| Arrange: Position > Rotate Left 90, Rotate Right 90 | S19 | rotL rotR | present | present | | P0 |
| Arrange: Position > Flip Horizontal / Vertical | S19 | flipH flipV | present | present | | P0 |
| Arrange: Bring to Front / Send to Back | S1 | zFront zBack | present | present | | P0 |
| Arrange: Bring Forward / Send Backward | unverified | zFwd zBwd | present | present | | P1 |
| Arrange: Group / Ungroup | S1 | grp ungrp | present | present | Grouped shapes are read-only for edits on desktop ("Grouped: read-only in this slice") | P0 |
| Arrange: same width / height / size | unverified | sameW sameH sameWH | present | present | To the first shape selected; connectors and group members are left out | P2 |
| Arrange: reverse connector ends | unverified | reverseEnds | present | present | | P2 |
| Arrange: lock | unverified | lockShape | missing | present | | P2 |
| Arrange: shape operations (Union, Combine, Fragment, Intersect, Subtract, Join, Trim) | S18 for Trim (Developer tab, Shape Design, Operations); the others unverified | opUnion opCombine opFragment opIntersect opSubtract opJoin opTrim | present | present | Visio puts them on Developer (S18); LayoutCraft shows them in Home > Arrange. Engine: `shape_operation`; follow Visio's place | P1 |
| Editing: Select (Area Select, Lasso Select, Select All, Select by Type) | S19 | - | partial | partial | Rectangle drag and Ctrl+A exist; lasso and by-type missing | P1 |
| Editing: Layers > Assign to Layer, Layer Properties | S22 | layersBtn | partial | present | Desktop: Layers window and inspector (visible, prints, locked, membership, rename, colour); no Active / Snap / Glue | P2 |
| Editing: Change Shape | unverified | changeShapeBtn | present | present | Desktop: the button, then a click in the Shapes panel, or the panel's context menu | P2 |
| Editing: Find | unverified | findBtn | present | present | Desktop: finds and replaces on the current page | P2 |
| Editing: Styles | unverified | stylesBtn | partial | present | Desktop: applies a style of the drawing to the selection, saves a new one (name, line and fill colour, weight, text colour, size, bold, italic) and deletes one made for the drawing; no base style or paragraph settings, no loading a style into the form to change it | P2 |
| Duplicate (Ctrl+D is a web shortcut) | S1 | dup | present | present | Not a Visio Home button (unverified) | P1 |
| Delete | S1 (Delete key) | del | present | present | | P0 |

### 3.4 Insert

| Visio command | Src | LC id | Desktop | Browser | Notes | Prio |
|---|---|---|---|---|---|---|
| Pages: new / duplicate / rename / delete / reorder (Reorder Pages dialog Ctrl+Alt+P; Page dialog Shift+F4) | S1 | pageTools, pageAdd pageDup pageRename pageDel pageLeft pageRight | present | present | Desktop ribbon items exist; no Reorder dialog | P0 |
| Pages: background page | S5 | - | partial | partial | Desktop: a page tab's menu makes a background page; Page setup chooses which one shows beneath a page and makes one ordinary again. A background page itself cannot be shown for editing | P2 |
| Illustrations: Pictures | S25 snippet | insertPicture, pictureFile | missing | present | Engine `insert_picture` | P1 |
| Illustrations: Icons | S25 snippet (Visio 2016 book) | - | missing | missing | Needs our own icon set | P3 |
| Diagram Parts: Container | S20 | - | missing | missing | | P2 |
| Diagram Parts: Callout | S25 | - | missing | missing | | P2 |
| Diagram Parts: Connector | S25 snippet | connectTool | present | present | Same tool as Home | P2 |
| Links: Hyperlink | S25 snippet (Links group) | insertHyperlink | present | present | Desktop: Hyperlinks window (address, sub-address, description) | P1 |
| Text: Text Box | S25 snippet (Text group) | insertTextBox | present | present | | P1 |
| Text: Field, Symbol | unverified | - | missing | missing | | P3 |

### 3.5 Draw

| Visio command | Src | LC id | Desktop | Browser | Notes | Prio |
|---|---|---|---|---|---|---|
| Draw tab (ink: pens, eraser, lasso, ink to text) | unverified (no cited page; LayoutCraft notes the tab as not built) | - | missing | missing | Defer; touch-only audience | P3 |

### 3.6 Design

| Visio command | Src | LC id | Desktop | Browser | Notes | Prio |
|---|---|---|---|---|---|---|
| Page Setup: dialog (page size, orientation) | S4 (orientation and size under Design); dialog also from the page-tab menu | pageSetup | present | present | | P0 |
| Page Setup: Orientation | S4 | designOrientation | present | present | | P1 |
| Page Setup: Size | S4 | designSize | present | present | | P1 |
| Themes (hover preview) | S8 | themeGallery | partial | present | Engine `themes_json`; desktop Design tab shows themes and the four variants as pictures drawn by our renderer (the theme list with names stays beside them); resting the pointer or the keyboard focus on one shows the shown page that way until it leaves; not compared with Visio by hand | P1 |
| Variants | S8 | variantGallery | present | present | | P2 |
| Backgrounds | S8 | paperColor | missing | partial | Visio adds a background page (S5, S8); LC sets a colour | P2 |
| Borders & Titles | S8 | borderStyle borderTitle borderApply borderRemove | present | present | | P2 |
| Layout: Re-Layout Page (gallery: flowchart, compact tree, radial, circular, more options) | S23 | - | missing | missing | New engine work; see 5, batch 7 | P2 |
| Layout: Connectors style | S4 | - | missing | missing | | P2 |

### 3.7 Data

| Visio command | Src | LC id | Desktop | Browser | Notes | Prio |
|---|---|---|---|---|---|---|
| Show/Hide: Shape Data window | S5 | dataBtn | partial | present | Desktop: window for one shape: values, add and remove fields, set and change a field's type (text, yes or no, list) and prompt; hide or show a field; no number or date type, no renaming, no order editing | P1 |
| External Data: Link Data to Shapes | S24 | - | missing | missing | Needs an import path | P3 |
| Refresh All / AutoLink | S24 | - | missing | missing | | P3 |
| Display Data: Data Graphics | S4, S5 | - | missing | missing | | P3 |
| Display Data: Insert Legend | S24 | - | missing | missing | | P3 |

### 3.8 Process

| Visio command | Src | LC id | Desktop | Browser | Notes | Prio |
|---|---|---|---|---|---|---|
| Create sub-process | S4 | - | missing | missing | | P3 |
| Check Diagram (validation against rules) | S4 (validation named generally) | checkDiagram | present | present | | P2 |
| Import / export workflow | S4 | - | missing | missing | | P3 |

### 3.9 Review

| Visio command | Src | LC id | Desktop | Browser | Notes | Prio |
|---|---|---|---|---|---|---|
| Comments: New Comment | S15 | newComment | present | present | Desktop: the Comments window adds a comment to the page or the selected shape | P1 |
| Comments: Comments Pane | S15 | showComments | partial | present | Desktop: a window that lists, changes and deletes comments; no replies, not docked | P1 |
| Markup: Track Markup | S15 (not on the tab by default) | - | missing | missing | | P3 |
| Proofing: spelling | S4 | - | missing | partial | Browser relies on the text editor's own check | P3 |
| Language | S4 | - | missing | missing | | P3 |
| Reports | unverified | reportBtn | present | present | LayoutCraft inventory report | P3 |

### 3.10 View

| Visio command | Src | LC id | Desktop | Browser | Notes | Prio |
|---|---|---|---|---|---|---|
| Views: Full Screen (F5, Esc exits) | S1, S4 | fullScreen | present | present | Desktop keeps the status bar | P1 |
| Views: Normal / slide-show style views | unverified | fixedPage | missing | present | | P3 |
| Views: print preview | unverified | print | missing | present | | P2 |
| Show: Ruler | S26 | showRulers | present | present | Desktop counts on the printed page; a drawing scale is not applied (the ruler says so) | P0 |
| Show: Grid | S21 | showGrid | present | present | Ruler & Grid dialog (fixed / variable spacing, origin) missing | P0 |
| Show: Guides | S26 snippet | showGuides | present | present | Desktop: drag from a ruler, move, drop on a ruler or press Delete; mouse only for creating and moving; no turned guides | P0 |
| Show: Page Breaks | unverified | showBreaks | missing | present | | P2 |
| Show: Task Panes menu | S12, S11 | paneShapes paneSizePos paneSheet panePanZoom | partial | present | Desktop: Shapes pane, Size and Position (goes to the inspector's fields), Shape Formulas and an Inspector switch from the ribbon (the inspector is closed until asked for, also by F3 and F8); no Pan & Zoom, not grouped in one menu | P0 |
| Zoom: Zoom (dialog) | S11 | zl | present | present | Desktop: the Zoom menu (fixed levels, fit, page width) and a Custom zoom window for a typed percentage | P1 |
| Zoom: Fit Page to Window (Ctrl+Shift+W) | S11, S1 | fit | present | present | | P0 |
| Zoom: 100%, Page Width | unverified | zoom100 zoomWidth | present | present | | P2 |
| Zoom: zoom in / out (Alt+F6, Alt+Shift+F6 desktop; Ctrl+Plus/Minus web) | S1 | zin zout | present | present | LC uses Ctrl+= / Ctrl+- (web style) | P1 |
| Visual Aids: Snap & Glue (dialog Alt+F9) | S1 | snapGrid snapShapes snapGuides gridStep | partial | present | Desktop: window (Alt+F9) for what is snapped to, the snap strength and the grid; no glue settings | P1 |
| Visual Aids: Dynamic Grid (alignment, spacing, resizing guides; snap strength) | S10 | snapShapes | partial | partial | Desktop snaps to shapes and to equal spacing between two neighbours, drawing a guide through the centre and, while shapes are moved, a mark across each of two equal gaps; no repeated-spacing marks, no resize guides; one snap strength, in the Snap and Attach window | P0 |
| Visual Aids: AutoConnect | S6, S4 | autoConnectToggle | partial | present | Desktop: arrows add the picked shape or a copy and connect it; hovering an arrow lists up to four shapes of the open stencil by name | P0 |
| Window: New Window | unverified | newWindow | missing | present | | P3 |
| Window: Tile, Cascade, Switch Windows (Shift+F7, Ctrl+Shift+F7) | S1 | - | missing | missing | Single-document app | P3 |
| Macros | S19, S14 | - | missing | missing | Out of scope | P3 |
| Palette selection, tooltip delay | none | paletteSel tipDelay | missing | present | LayoutCraft additions | P3 |

### 3.11 Developer (hidden by default in Visio; S14)

| Visio command | Src | LC id | Desktop | Browser | Notes | Prio |
|---|---|---|---|---|---|---|
| Shape Design: Show ShapeSheet (shape, page, document) | S14 | paneSheet | partial | partial | Read only, shapes only; sits in View, not Developer | P2 |
| Shape Design: Geometry (Freeform, Arc, Pencil) | S14 | drawTools | missing | present | | P3 |
| Shape Design: Shape Name | S14 | - | present | present | Desktop: the Name field of the inspector | P3 |
| Shape Design: Behavior, Protection | S14 | - | missing | missing | | P3 |
| Shape Design: Operations (Trim and the other operations) | S18 | opUnion ... opTrim | partial | present | Desktop has them in Home > Arrange; there is no Developer tab | P2 |
| Stencil: new stencil | S14 | - | missing | missing | Needs stencil writer | P3 |
| Show/Hide: Document Stencil, Drawing Explorer, Master Explorer | S14 | - | missing | missing | | P3 |
| Code & Controls: Macros, Visual Basic | S14 | - | missing | missing | Not planned | P3 |

### 3.12 Help

| Visio command | Src | LC id | Desktop | Browser | Notes | Prio |
|---|---|---|---|---|---|---|
| Help pane (F1) | S1 | helpShortcuts | partial | partial | Ctrl+F1 is collapse in LC; desktop F1 opens the shortcut list (no help pane) | P1 |
| About | unverified | helpAbout | present | present | | P2 |
| Tell me / search box for commands | S27 (a search bar is new in 2024) | - | missing | missing | Command search | P2 |

### 3.13 Contextual tabs

| Visio command | Src | LC id | Desktop | Browser | Notes | Prio |
|---|---|---|---|---|---|---|
| Container Format tab (margins, resize, select contents, lock, disband) | S20 | - | missing | missing | Needs Containers first | P2 |
| Plan tab (office layout template) | S5 | - | missing | missing | Template specific | P3 |
| Picture, connector, text contextual tabs | unverified | - | missing | missing | | P3 |

### 3.14 LayoutCraft commands Visio does not have in the sources above

Compare File (`compareBtn`: present on both), AI assistant info (`mcpInfo`: present on both). Keep, outside the Visio-style groups
(Review and Help respectively); not counted in the parity totals.

## 4. Interaction conventions

| Convention | Visio (source) | LayoutCraft desktop | LayoutCraft browser | Gap / prio |
|---|---|---|---|---|
| Selection by click, Shift/Ctrl add, Shift-click deselects, Esc or blank click clears | S19 | Click; Shift- or Ctrl-click adds or removes; Shift / Ctrl band adds; Tab through shapes; Esc clears | Same, plus Tab through shapes (`tabThroughShapes`) | Desktop matches |
| Area select and lasso, optional "select shapes partially within area" | S19 | Rectangle drag only, fully inside | Rectangle drag | Lasso; partial-in-area option: P2 |
| Handles: blue corner handles resize proportionally, side handles change one dimension, rotation handle round above shape, yellow control handles, 1-D endpoints | S5, S19 | Corner and side resize handles, rotate handle above top edge (`ROTATE_HANDLE`), Shift snaps rotation to 15 degrees; connector end handles; no control handles; rotated shapes cannot be resized ("move only") | Same plus Edit Points | Control handles; resize of rotated shapes: P1 |
| Rotate around the pin; finer steps farther from the selection | S19 | Rotates about pin in engine; no distance-based fine step | Same | P3 |
| Move by drag, Shift constrains to an axis, drag to another page tab moves the shape | S19 | Drag; Shift pressed during the drag keeps it to one axis; no page-tab drop | Drag | Page-tab drop P3 |
| Nudge: arrows move to the next snap position, Shift+arrow one pixel | S19, S1 | 0.05 in step, Shift 0.5 in (`nudge_delta`) | Same | Reverse the Shift meaning: P1 |
| Glue: connectors glue to shape or a connection point; connection points drawn on hover | S6 (caption mentions a point connection at each end) | Release on a shape glues to the whole shape or to a connection point within reach; end handles can be dragged | Same plus Connection Point tool | Glue indicators; per-layer Glue switch: P1 |
| Dynamic Grid: guides while moving near other shapes or page/container margins; spacing guides; snap strength in Visual Aids dialog | S10 | Snap to grid / shapes / guides and to equal spacing between neighbours, with a drawn guide line; desktop also marks the two equal gaps | Same | Repeated spacing and resize guides; snap strength in the browser; margin guides: P0 |
| Snap & Glue dialog (what snaps, what glues) | S1 | Toggles in View and the empty-page menu, and a window (Alt+F9) | Same | Glue settings: P1 |
| AutoConnect: pointing at a shape shows four arrows; hovering an arrow shows a mini toolbar of up to four Quick Shapes; click adds and connects; dropping a Shapes-window item on an arrow also works; per-file setting, toggled in View | S3, S6, S7 | Arrows place the picked stencil shape or a copy of the shape and connect; hovering an arrow lists the first four shapes of the open stencil (pointer only); a shape dragged from the panel onto an arrow is placed and connected; toggled in View | Arrows place the picked stencil shape or a copy of the shape and connect | Desktop: keyboard use, thumbnails, per-file setting. Browser: the shape picker |
| Drag from stencil: drag a master onto the page creates an instance; the master stays | S5, S2 | Drag from pane (ghost label), click-then-click placement, Esc cancels | Drag | Done, plus drop on a shape to glue/replace: P3 |
| Right-click menus | S5, S9 | See section 2 | See section 2 | P1 |
| Text editing: select a shape and type starts editing; Esc or a click outside ends; F2 toggles text edit and selection; Ctrl+A in text selects all text | S5, S1 | F2, Ctrl+E or typing on one selected shape open a text window (typed characters are added after the text); Ctrl+Enter or F2 applies, Esc discards | F2, type-to-edit unverified | Type-to-edit: P0 |
| Text Block tool moves the text block independently | S5 | none | none | P2 |
| Zoom: Ctrl+wheel; plain wheel scrolls | S11 | Ctrl+wheel zooms, wheel scrolls | wheel scrolls; `wheelZoom` option | Desktop matches |
| Pan: Pan & Zoom window; scroll; Ctrl+Shift right-drag | S11 | middle button, Space-drag, Pan tool, Ctrl+Shift right-drag | same plus pane | P2 |
| Tab / Shift+Tab cycle focus through shapes; Enter selects the focused shape | S1 | Tab / Shift+Tab select the next / previous shape once a shape is selected on the canvas | `tabThroughShapes` | Desktop: no keyboard way in without a selection (Ctrl+A first) |
| Page change: Page Up / Page Down in full screen; Ctrl+Page Down next page | S1 | both | none | Desktop matches |

### 4.1 Keyboard shortcuts: Visio desktop (S1) against LayoutCraft desktop (`main.rs`)

Status: `same` the key does the same thing, `diff` LayoutCraft binds the key to something else or binds a different key to the
same action, `none` not bound. Browser column from `web/shell.js` and `web/keyboard.js`.

| Action | Visio desktop key (S1) | LayoutCraft desktop | LayoutCraft browser | Result |
|---|---|---|---|---|
| Group | Ctrl+G or Ctrl+Shift+G | Ctrl+G | Ctrl+G | same |
| Ungroup | Ctrl+Shift+U | Ctrl+Shift+U | Ctrl+Shift+G | same on desktop |
| Bring to front | Ctrl+Shift+F | Ctrl+Shift+F | Ctrl+Shift+F | same |
| Send to back | Ctrl+Shift+B | Ctrl+Shift+B | Ctrl+Shift+B | same |
| Rotate left / right | Ctrl+L / Ctrl+R | Ctrl+L / Ctrl+R | `[` / `]` | same on desktop |
| Flip horizontal / vertical | Ctrl+H / Ctrl+J | Ctrl+H / Ctrl+J | `H` / `V` | same on desktop |
| Align Shapes dialog | F8 | F8 goes to the inspector's align buttons (no dialog) | none | partial |
| Pointer tool | Ctrl+1 | Ctrl+1 | Esc | same on desktop |
| Text tool | Ctrl+2 | Ctrl+2 (draws a text box) | none | same on desktop |
| Connector tool | Ctrl+3 | Ctrl+3 | none | same on desktop |
| Connection point tool | Ctrl+Shift+1 | Ctrl+Shift+1 | none | same on desktop |
| Text block tool | Ctrl+Shift+4 | none | none | none |
| Crop tool | Ctrl+Shift+2 | none | none | none |
| Pencil / Freeform / Line / Arc / Rectangle / Ellipse | Ctrl+4 / 5 / 6 / 7 / 8 / 9 | Ctrl+6 / 8 / 9 (line, rectangle, ellipse) | none | partial |
| Format Painter | Ctrl+Shift+P | none | none | none |
| Format Shape pane | F3 | F3 goes to the inspector's Style settings (no pane of its own) | none | partial |
| Text dialog | F11 | none | none | none |
| Snap & Glue dialog | Alt+F9 | Alt+F9 (no glue settings) | none | same on desktop |
| Full-screen view / exit | F5 / Esc | F5 / Esc | no key | same on desktop |
| Fit page to window | Ctrl+Shift+W | Ctrl+Shift+W | none | same on desktop |
| Zoom in / out | Alt+F6 / Alt+Shift+F6 (web: Ctrl+Plus / Ctrl+Minus) | Alt+F6 / Alt+Shift+F6, also Ctrl+= or Ctrl++ / Ctrl+- | Ctrl+= or Ctrl++ / Ctrl+- | same on desktop |
| Cycle focus through shapes | Tab / Shift+Tab | Tab / Shift+Tab with a shape selected | Tab on the stage | same on desktop |
| Toggle text edit and selection | F2 | F2 (also Ctrl+E) | F2 | same |
| Nudge | Arrow (to snap position); Shift+Arrow one pixel | Arrow 0.05 in; Shift+Arrow 0.5 in | same | diff |
| Select all | Ctrl+A (web table; desktop page lists it for text and masters) | Ctrl+A | Ctrl+A | same |
| Duplicate | Ctrl+D (web table) | Ctrl+D | Ctrl+D | same |
| Cut / copy / paste, undo | standard; Ctrl+Y redo in the web table | Ctrl+X/C/V, Ctrl+Z; redo Ctrl+Y or Ctrl+Shift+Z | same | same |
| Page dialog / reorder pages | Shift+F4 / Ctrl+Alt+P | none | none | none |
| Next / previous page | Ctrl+Page Down / Ctrl+Page Up | same | none | same on desktop |
| Switch to next drawing | Ctrl+Tab or Ctrl+F6 | none | none | none |
| Move to a task pane | F6 | none | F6 cycles regions | none (desktop) |
| Help pane | F1 | F1 opens the shortcut list (Ctrl+F1 collapses the ribbon) | none | partial |
| Key tips | Alt, then letters (Alt+H is Home) | none on desktop | Alt shows tips | none (desktop) |
| Open context menu from keyboard | Shift+F10 | none | none | none |
| Bold / italic / underline | Ctrl+B / I / U | Ctrl+B / I / U (selected shapes) | Ctrl+B / I / U | same on desktop |
| Text alignment | Ctrl+Shift+L / C / R / J (web table uses S for centre) | Ctrl+Shift+L / C / R / J | none | same on desktop |
| Edit text stop | Esc | Esc | Esc / Ctrl+Enter | same |
| Delete | Delete | Delete or Backspace | same | same |
| Save | Ctrl+S (assumed standard, not in S1) | Ctrl+S (saves a copy) | Ctrl+S | partial |
| Find | Ctrl+F (web `findBtn`) | Ctrl+F | Ctrl+F | same on desktop |
| Hyperlink | Ctrl+K (unverified in S1) | Ctrl+K | Ctrl+K | same on desktop |

Count of the 41 rows above: same 26, diff 1, partial 5, none 9. This counts table rows, not Microsoft's whole list.

### 4.2 Keyboard conflicts to resolve

Four LayoutCraft keys collide with the Visio meaning of keys the same host may want later: `H` and `V` (plain letters flip a shape;
Visio uses them for nothing in S1 but uses Ctrl+H and Ctrl+J), `[` and `]` (rotate; Visio uses Ctrl+L and Ctrl+R) and Ctrl+Shift+G (Visio
groups with it). On the desktop the Visio keys are the bindings; the earlier plain keys `H`, `V`, `[` and `]` were removed by the maintainer's decision, so that typing can edit text (Ctrl+Shift+G now groups);
the browser shell is unchanged. The Visio keys should become the primary bindings and the current ones either removed or kept as secondary
aliases for one release, announced in the shortcut dialog.

### 4.3 Key tips

Alt then a letter per tab is documented only for Alt+H (S1). The key-tip letters in `web/keyboard.js` for the other tabs, and the
comment in that file describing where they came from, rest on something other than a public page cited here. Before the desktop app
copies them, re-derive them from a public source or choose our own letters and mark them as such.

## 5. Roadmap

Each batch is one reasonable pull request. Hour figures are estimates. Batches 1 to 3 start with commands the browser shell already
has and the desktop lacks, where engine logic exists in Rust and the host only has to call and draw (`AGENTS.md` rule 8). Batch 0
is small, protective work that the rest depends on.

| Batch | Contents | Why here | Est. h |
|---|---|---|---|
| 0. Naming and keys | Change user-visible labels to the names in section 6 (labels only, ids kept, old labels as hidden search aliases); move shortcuts to the Visio keys (4.1, 4.2); add F1, F3 (placeholder), Ctrl+1/2/3, Ctrl+Shift+P bindings as the features arrive. | Cheap; avoids re-doing labels per feature. | 10-16 |
| 1. Desktop Home tab from the web ribbon | Font and Paragraph groups (engine op 13 and `text_state_json`), Fill/Line/Effects ribbon controls, Format Painter, Edit Points, Fit Curve, Connection Point tool, draw tools (Rectangle, Ellipse, Line, Arc, Freeform, Pencil: engine `draw*`), same width/height, reverse ends, lock, shape operations (engine `shape_operation`), Find, Change Shape button, Duplicate. | 22 browser commands with Rust support that desktop shows disabled. | 40-60 |
| 2. Desktop Insert, Design and Data | Pictures (`insert_picture`), Hyperlink (`hyperlinks_json`), Themes and Variants (`themes_json`), Orientation and Size, Shape Data window (ops 17/18), Layers launcher. | Same reason; mostly dialogs. | 28-40 |
| 3. Desktop View tab | Rulers, ruler guides (create / move / delete; the engine already stores guides), Guides, Page Breaks, Full Screen, Page Width, Zoom dialog, Print, Size & Position editable pane, Pan & Zoom pane, Shape formula table (read only), New Window. | Large visible gap on the canvas. | 40-60 |
| 4. Window frame | Page tab strip with right-click menu and drag reorder, status bar with zoom slider, Fit button and shape-geometry items, Ctrl+wheel zoom default, Tab focus cycle, F6 regions, Shift+F10. | Layout parity; needs no new engine code. | 30-44 |
| 5. AutoConnect and Quick Shapes | Connect arrows on hover, mini toolbar picker, the frequent-shapes area in each stencil and a generated frequent-shapes list, accordion Shapes window with a "More shapes" menu and minimise-to-column. | The largest convention gap (section 4). | 36-56 |
| 6. Dynamic guides and Snap dialog | Spacing and resize guides, margin and container guides, snap strength, Snap & Glue dialog, per-layer snap and glue, Ruler & Grid dialog. | Builds on 3 and 4. | 24-36 |
| 7. Backstage and options | Full-page File view on desktop, template gallery, Options page, Customize Ribbon and Quick Access Toolbar customisation, Developer tab. | Mostly UI; the options store is new. | 40-60 |
| 8. Contextual tabs, containers, callouts | Contextual-tab mechanism, Container Format tab, Containers and Callouts, Format Shape pane with tabs, selection mini toolbar. | Needs engine work (container membership). | 60-90 |
| 9. Layout and data | Re-layout page, connector styles, Link Data to Shapes, data visuals, legend. | New engine features; later. | 80-140 |
| 10. Retire the browser shell | After desktop parity of batches 1 to 4, move remaining browser-only features and delete `web/`. | Decision outside this document. | not estimated |

Rough total for batches 0 to 9: 350 to 560 hours (estimate).

## 6. Naming

A name shown to users must be ours (`naming.md`). Command ids, code and this internal document may keep Microsoft's names. Each
proposal needs the check in `naming.md` rule 4 before use: the owner's trademark list, a register search, and three unrelated
products that use a descriptive name. None has been run for this document.

| Microsoft name (as found) | Source | Proposed LayoutCraft name | Status |
|---|---|---|---|
| Visio, .vsdx (descriptive use only) | all | keep for "opens .vsdx files" statements only; never in a feature label | needs naming check |
| ShapeSheet | S14 | Shape Formulas | needs naming check |
| AutoConnect | S4, S6 | Add and Connect Arrows | needs naming check |
| Quick Shapes | S3 | Frequent Shapes | needs naming check |
| Dynamic Grid | S4, S10 | Live Alignment Guides | needs naming check |
| Data Graphics | S4, S5 | Data Visuals | needs naming check |
| SmartShapes (not found in a cited page; brand family) | unverified | Shape Behaviours (if needed at all) | needs naming check |
| Shape Data | S5 | Shape Fields | needs naming check |
| Shapes window / stencil / master | S2, S5 | Shapes panel / shape library / library shape | needs naming check (stencil is also the `.vssx` file type name; keep it as a file-type word) |
| Size & Position | S12 | Size and Position | needs naming check |
| Pan & Zoom | S11 | Page Overview | needs naming check |
| Format Shape pane | S9 | Shape Format panel | needs naming check |
| Format Painter | S1 | Copy Format | needs naming check (LayoutCraft's ribbon labels it "Format Painter" today in `ribbon.rs`) |
| Quick Access Toolbar | S16 | Favourite Commands Bar | needs naming check |
| Backstage | not in a cited page | File page | needs naming check |
| Key Tips | S1 | Key hints | needs naming check |
| Tell me (search box) | S27 (only "search bar") | Command Search | needs naming check |
| Mini toolbar | S5 | Floating Toolbar | needs naming check |
| Task panes | S12 | Side panels | needs naming check |
| Auto Align & Space | S13 | Tidy Alignment and Spacing | needs naming check |
| Re-Layout Page | S23 | Rearrange Page | needs naming check |
| Snap & Glue | S1 | Snap and Attach | needs naming check |
| Visual Aids | S10 | Guides and Aids | needs naming check |
| Container, Callout, List | S20, S25 | keep (ordinary words) | needs naming check |
| Layers, Group, Align, Distribute, Position, Pointer tool, Text Block | S1, S13, S22 | keep (ordinary words, as `naming.md` lists Layers) | needs naming check (low risk) |
| Check Diagram | S4 (validation only; the exact label is not in S4) | Check Drawing | needs naming check (LayoutCraft ribbon label today: "Check Diagram") |
| Change Shape, Connection Point, Edit Points | unverified | Replace Shape, Glue Point, Edit Vertices | needs naming check (current LayoutCraft labels in `ribbon.rs` match Visio's wording as far as known) |
| Developer tab | S14 | Developer | needs naming check (ordinary word) |
| Process tab | S4 | Flow | needs naming check |

## 7. Open points and risks

1. This is a planning document; hour totals are estimates and the browser shell status is based on code references, not hand
   testing.
2. Parts of the Visio side rest on search snippets and 2009-era pages (S4, S14); verify the group lists against a public page that
   shows the current ribbon before building a group.
3. Two existing LayoutCraft labels (`Format Painter`, `Check Diagram`) and a header comment in `web/keyboard.js` should be reviewed
   under `naming.md` and the provenance rules (see 4.3 and section 6).
4. Visio on the web lacks some desktop features (layers, grid snapping in the free tier, containers; S21, S22, S20). Parity targets
   the desktop edition unless a row says otherwise.
