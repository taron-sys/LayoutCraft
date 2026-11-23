// UI chrome: shape palette, format inspector, toolbar & page tabs.
import { SHAPES, SHAPE_GROUPS, shapePreview } from './shapes.js';
import { iconLevel, dataColorFill } from './model.js';
import { clamp } from './util.js';

const $ = (sel, root = document) => root.querySelector(sel);

export function buildPalette(app) {
  const host = $('#paletteItems');
  host.innerHTML = '';
  for (const group of SHAPE_GROUPS) {
    const g = document.createElement('div');
    g.className = 'pgroup';
    g.innerHTML = `<div class="pgroup-title">${group}</div>`;
    const grid = document.createElement('div');
    grid.className = 'pgrid';
    for (const [type, def] of Object.entries(SHAPES)) {
      if (def.group !== group) continue;
      const b = document.createElement('div');
      b.className = 'shape-btn';
      b.draggable = true;
      b.dataset.shape = type;
      b.title = def.name;
      b.innerHTML = shapePreview(type);
      grid.appendChild(b);
    }
    g.appendChild(grid);
    host.appendChild(g);
  }

  host.addEventListener('click', (e) => {
    const b = e.target.closest('.shape-btn');
    if (!b) return;
    app.setTool('shape', b.dataset.shape);
  });

  host.addEventListener('dragstart', (e) => {
    const b = e.target.closest('.shape-btn');
    if (!b) return;
    e.dataTransfer.setData('application/x-layoutcraft-shape', b.dataset.shape);
    e.dataTransfer.effectAllowed = 'copy';
  });
}

export function buildRecent(app) {
  const host = $('#paletteRecent');
  host.innerHTML = '';
  for (const type of app.state.recent.slice(0, 8)) {
    const b = document.createElement('div');
    b.className = 'shape-btn';
    b.draggable = true;
    b.dataset.shape = type;
    b.title = SHAPES[type]?.name || type;
    b.innerHTML = shapePreview(type);
    host.appendChild(b);
  }
  host.addEventListener('click', (e) => {
    const b = e.target.closest('.shape-btn');
    if (b) app.setTool('shape', b.dataset.shape);
  });
  host.addEventListener('dragstart', (e) => {
    const b = e.target.closest('.shape-btn');
    if (!b) return;
    e.dataTransfer.setData('application/x-layoutcraft-shape', b.dataset.shape);
    e.dataTransfer.effectAllowed = 'copy';
  });
}

// ------------------------------------------------------------- inspector --
let inspKey = null;

export function updateInspector(app) {
  const host = $('#inspectorBody');
  const sel = [...app.state.selection];
  const pg = app.getPage();
  const shapes = pg.shapes.filter((s) => sel.includes(s.id));
  const conns = pg.connectors.filter((c) => sel.includes(c.id));
  // Grouping/ungrouping, adding a member or deleting one changes what the
  // Group section reports even when the selection IDs stay the same.
  const groupOf = (o) => (o.group ? `${o.group.id}:${o.group.rootId}` : '-');
  const memberCount = (o) => {
    if (!o.group) return 0;
    const root = String(o.group.rootId || o.group.id);
    return [...pg.shapes, ...pg.connectors]
      .filter((m) => m.group && String(m.group.rootId) === root).length;
  };
  const groupSig = [...shapes, ...conns]
    .map((o) => `${groupOf(o)}/${memberCount(o)}`).join(',');
  const cff = app.swimlaneContext();
  const cffSig = cff
    ? [cff.pool, cff.list, cff.phaseList, cff.lane]
      .map((c) => (c ? `${c.modelId || c.visioId}:${(c.items || []).length}:${c.heading || c.label || ''}` : '-'))
      .join('|')
    : '-';
  const richSig = shapes.map((s) => (s.rich
    ? s.rich.paragraphs.map((p) => `${p.align || ''}:${p.bullet ? 1 : 0}:${(p.runs || []).length}`).join(',')
    : '-')).join('|');
  const dataSig = shapes.map((s) => (s.data || []).length).join(',');
  const calloutSig = shapes.map((s) => (s.callout
    ? `${s.callout.kind || 'text'}:${s.callout.field}:${s.callout.position}:${s.attachedTo || ''}:${s.callout.autoSize === false ? 0 : 1}:${s.callout.min ?? ''}:${s.callout.max ?? ''}:${s.callout.showValue === false ? 0 : 1}:${s.callout.iconSet || ''}:${s.callout.kind === 'icon' ? iconLevel(s, app.getPage()) : ''}`
    : '-')).join(',');
  const attachedSig = shapes.map((s) => app.calloutsFor
    ? app.calloutsFor([s.id]).map((c) => `${c.id}:${c.callout.field}:${c.callout.position}`).join('|')
    : '').join(';');
  const fitSig = shapes.map((s) => (s.textFit ? `${s.textFit.width ? 1 : 0}${s.textFit.height ? 1 : 0}` : '-')).join(',');
  const linkSig = shapes.map((s) => (s.hyperlinks || []).length).join(',');
  const imageSig = shapes.map((s) => {
    if (!s.image) return '-';
    const c = app.imageCropOf ? app.imageCropOf(s.image) : { left: 0, top: 0, right: 0, bottom: 0 };
    return [s.image.mediaId, s.image.brightness || 0, s.image.contrast || 0,
      s.image.gamma || 1, s.image.transparency || 0,
      c.left, c.top, c.right, c.bottom].map((v) => (typeof v === 'number' ? Math.round(v * 100) / 100 : v)).join(':');
  }).join(',');
  const colorSig = shapes.map((s) => (s.dataColor
    ? `${s.dataColor.field}:${s.dataColor.min}:${s.dataColor.max}:${s.dataColor.low}:${s.dataColor.mid}:${s.dataColor.high}:${dataColorFill(s) || ''}`
    : '-')).join(',');
  const theme = app.doc.theme || {};
  const themeSig = `${theme.name || ''}:${theme.activeVariation || 0}:${app.state.quickStyle || ''}:${app.state.themeScope || ''}`;
  const key = `${sel.slice().sort().join(',')}|${shapes.length}|${conns.length}|${groupSig}|${cffSig}|${richSig}|${dataSig}|${calloutSig}|${attachedSig}|${app.state.dgField || ''}|${fitSig}|${linkSig}|${imageSig}|${themeSig}|${colorSig}|${app.doc.pageIndex}`;
  if (key !== inspKey) {
    inspKey = key;
    host.innerHTML = buildInspectorHTML(app, shapes, conns);
    // The inspector body is rebuilt often; attach its delegated listeners
    // once or every action fires once per rebuild.
    if (!host.__wired) {
      wireInspector(app, host);
      host.__wired = true;
    }
  }
  patchInspector(app, host, shapes, conns);
}

function themeSectionHTML(app) {
  const theme = app.ensureTheme();
  const presets = Object.keys(app.themePresets || {});
  const current = theme.name || 'Office';
  const variations = theme.variations || [];
  const active = theme.activeVariation || 0;
  const scope = app.state.themeScope || 'selection';
  const quick = app.state.quickStyle || '';
  return `<div class="insp-section">
    <div class="insp-title">Theme</div>
    <div class="insp-row"><label>Theme</label>
      <select data-act="themePreset">
        ${presets.map((n) => `<option value="${escapeHtml(n)}" ${n === current ? 'selected' : ''}>${escapeHtml(n)}</option>`).join('')}
        ${presets.includes(current) ? '' : `<option value="__current" selected>${escapeHtml(current)}</option>`}
      </select></div>
    ${variations.length > 1 ? `<div class="insp-row"><label>Variation</label>
      <select data-act="themeVariation">
        ${variations.map((_, i) => `<option value="${i}" ${i === active ? 'selected' : ''}>Variation ${i + 1}</option>`).join('')}
      </select></div>` : ''}
    <div class="insp-row"><label>Apply to</label>
      <select data-act="themeScope">
        <option value="selection" ${scope === 'selection' ? 'selected' : ''}>Selection</option>
        <option value="page" ${scope === 'page' ? 'selected' : ''}>Page</option>
        <option value="document" ${scope === 'document' ? 'selected' : ''}>Document</option>
      </select></div>
    <div class="insp-grid" style="margin-top:6px">
      ${['subtle', 'moderate', 'intense', 'outline', 'plain'].map((s) =>
        `<button class="chip ${quick === s ? 'active' : ''}" data-act="quickStyle" data-val="${s}">${s}</button>`).join('')}
    </div>
  </div>`;
}

function buildInspectorHTML(app, shapes, conns) {
  if (!shapes.length && !conns.length) {
    return `<div class="insp-empty">
      <b style="color:#dfe4ec">Nothing selected</b><br><br>
      <kbd>V</kbd> select &nbsp; <kbd>C</kbd> connector<br>
      <kbd>R</kbd> rectangle &nbsp; <kbd>E</kbd> ellipse<br>
      <kbd>Del</kbd> delete &nbsp; <kbd>Ctrl</kbd>+<kbd>D</kbd> duplicate<br>
      <kbd>Ctrl</kbd>+<kbd>0</kbd> fit &nbsp; <kbd>F</kbd> fit<br><br>
      <b style="color:#dfe4ec">Connectors:</b> press <kbd>C</kbd> (or the
      Connect tool) and click the source shape, then the target shape. Select
      a connector to change its arrowheads.<br><br>
      Drag a shape from the palette, or double-click a shape to type into it.
      Use the picture button to insert an image (or drop one on the canvas).
      <div class="insp-grid" style="margin-top:10px">
        <button class="chip" data-act="relayout" data-val="vertical" title="Layered top-down layout">Layout ↓</button>
        <button class="chip" data-act="relayout" data-val="horizontal" title="Layered left-to-right layout">Layout →</button>
      </div>
    </div>` + themeSectionHTML(app);
  }

  let html = '';

  if (shapes.length === 1 && !conns.length) {
    const s = shapes[0];
    html += `<div class="insp-section">
      <div class="insp-title">Shape</div>
      <div class="insp-row"><label>Type</label>
        ${s.image
          ? '<span>Picture</span>'
          : s.callout
            ? `<span>${s.callout.kind === 'bar' ? 'Data bar'
              : s.callout.kind === 'icon' ? 'Data icon'
                : s.callout.kind === 'heading' ? 'Data heading' : 'Data callout'}</span>`
            : `<select data-act="type">${Object.entries(SHAPES)
              .filter(([, d]) => d.group === 'Basic Shapes' || d.group === 'Flowchart')
              .map(([k, d]) => `<option value="${k}" ${k === s.type ? 'selected' : ''}>${d.name}</option>`).join('')}
            </select>`}</div>
      <div class="insp-row">
        <input type="number" data-act="x" value="${round(s.x)}" title="X"/>
        <input type="number" data-act="y" value="${round(s.y)}" title="Y"/>
        <input type="number" data-act="w" value="${round(s.w)}" title="Width"/>
        <input type="number" data-act="h" value="${round(s.h)}" title="Height"/>
      </div>
      <div class="insp-row"><label>Rotate</label>
        <input type="number" data-act="rot" value="${round(s.rot || 0)}" step="15"/></div>
    </div>`;

    if (s.callout) {
      const isBar = s.callout.kind === 'bar';
      const isIcon = s.callout.kind === 'icon';
      const target = app.getPage().shapes.find((x) => x.id === s.attachedTo);
      const rows = (target && target.data) || [];
      const hasField = rows.some((d) => String(d.name) === String(s.callout.field));
      html += `<div class="insp-section">
        <div class="insp-title">${isBar ? 'Data bar'
          : isIcon ? 'Data icon'
            : s.callout.kind === 'heading' ? 'Data heading' : 'Data callout'}</div>
        <div class="insp-row"><label>Target</label><span>${escapeHtml(target ? (target.label || target.type) : 'missing target')}</span></div>
        <div class="insp-row"><label>Field</label>
          ${rows.length
            ? `<select data-act="calloutField">
                ${rows.map((d) => `<option value="${escapeHtml(d.name)}" ${String(d.name) === String(s.callout.field) ? 'selected' : ''}>${escapeHtml(d.label || d.name)}</option>`).join('')}
                ${hasField ? '' : `<option value="${escapeHtml(s.callout.field)}" selected>${escapeHtml(s.callout.field)}</option>`}
              </select>`
            : `<input type="text" data-act="calloutField" value="${escapeHtml(s.callout.field || '')}"/>`}
        </div>
        ${isIcon ? `<div class="insp-row"><label>Icon</label>
          <select data-act="iconSet">
            <option value="boxes" ${(s.callout.iconSet || 'boxes') === 'boxes' ? 'selected' : ''}>Boxes</option>
            <option value="diamonds" ${s.callout.iconSet === 'diamonds' ? 'selected' : ''}>Diamonds</option>
            <option value="circles" ${s.callout.iconSet === 'circles' ? 'selected' : ''}>Circles</option>
          </select>
          <span class="muted">Level ${iconLevel(s, app.getPage())} of 5</span></div>`
        : `<div class="insp-row"><label>Prefix</label>
          <input type="text" data-act="calloutLabel" value="${escapeHtml(s.callout.label || '')}" placeholder="e.g. Owner:"/></div>`}
        <div class="insp-row"><label>Position</label>
          <select data-act="calloutPosition">
            ${['top', 'bottom', 'left', 'right', 'free'].map((p) => `<option value="${p}" ${(s.callout.position || (isIcon ? 'left' : 'top')) === p ? 'selected' : ''}>${p}</option>`).join('')}
          </select></div>
        ${isBar || isIcon ? `<div class="insp-row"><label>Min / Max</label>
          <input type="number" data-act="barMin" value="${Number.isFinite(Number(s.callout.min)) ? s.callout.min : 0}"/>
          <input type="number" data-act="barMax" value="${Number.isFinite(Number(s.callout.max)) ? s.callout.max : 100}"/></div>` : ''}
        ${isBar ? `<div class="insp-row"><label style="flex:0 0 auto"><input type="checkbox" data-act="barShowValue" ${s.callout.showValue === false ? '' : 'checked'}/> Show value</label></div>` : ''}
        ${isBar ? '<div class="insp-row"><span class="muted">Fill is the bar; Line is the track.</span></div>' : ''}
        ${isIcon ? '<div class="insp-row"><span class="muted">Fill is the icon; Line is the outline. Five equal bands from min to max.</span></div>' : ''}
        <div class="insp-row">
          ${isIcon ? '' : `<label style="flex:0 0 auto"><input type="checkbox" data-act="calloutAuto" ${s.callout.autoSize === false ? '' : 'checked'}/> Auto size</label>`}
          <button class="chip" data-act="removeCallout">Remove</button>
        </div>
      </div>`;
    }

    if (s.image) {
      const media = app.doc.media && app.doc.media[s.image.mediaId];
      const crop = app.imageCropOf ? app.imageCropOf(s.image) : { left: 0, top: 0, right: 0, bottom: 0 };
      const noAdj = !!(media && media.adjust === false);
      const pct = (v) => Math.round((Number(v) || 0) * 100);
      const adj = (act, label, value, min, max, step, disabled = false) => `
        <div class="insp-row"><label>${label}</label>
          <input type="range" data-act="${act}" min="${min}" max="${max}" step="${step}" value="${value}" ${disabled ? 'disabled' : ''}/>
          <span class="img-val">${value}</span></div>`;
      html += `<div class="insp-section">
        <div class="insp-title">Picture</div>
        ${media && media.src && !media.unsupported
          ? `<div class="insp-row"><img class="img-preview" src="${media.src}" alt=""/></div>` : ''}
        <div class="insp-row"><span class="muted">${escapeHtml(media?.name || s.image.name || 'picture')}</span></div>
        ${(!media || media.unsupported)
          ? `<div class="insp-row"><span class="muted">Preview unavailable — the original ${escapeHtml((media?.ext || 'image').toUpperCase())} is preserved when saving.</span></div>`
          : ''}
        <div class="insp-grid" style="margin-top:6px">
          <button class="chip" data-act="imageReplace">Replace…</button>
          <button class="chip" data-act="imageFit" title="Resize the frame to the picture's natural size">Natural</button>
          <button class="chip" data-act="imageFrame" title="Clear the crop and fill the frame">Fit frame</button>
        </div>
        <div class="insp-title">Adjust</div>
        ${adj('imageBrightness', 'Bright', pct(s.image.brightness), -100, 100, 1, noAdj)}
        ${adj('imageContrast', 'Contrast', pct(s.image.contrast), -100, 100, 1, noAdj)}
        ${adj('imageGamma', 'Gamma', Math.round((s.image.gamma ?? 1) * 100) / 100, 0.1, 3, 0.05, noAdj)}
        ${adj('imageTransparency', 'Transp', pct(s.image.transparency), 0, 100, 1)}
        ${noAdj ? '<div class="insp-row"><span class="muted">Visio ignores brightness, contrast and gamma on indexed pictures (PNG-8 or GIF). Transparency still applies.</span></div>' : ''}
        <div class="insp-title">Crop %</div>
        <div class="insp-row"><label>L / T</label>
          <input type="number" data-act="cropLeft" min="0" max="94" value="${pct(crop.left)}" title="Crop left"/>
          <input type="number" data-act="cropTop" min="0" max="94" value="${pct(crop.top)}" title="Crop top"/></div>
        <div class="insp-row"><label>R / B</label>
          <input type="number" data-act="cropRight" min="0" max="94" value="${pct(crop.right)}" title="Crop right"/>
          <input type="number" data-act="cropBottom" min="0" max="94" value="${pct(crop.bottom)}" title="Crop bottom"/></div>
        <div class="insp-row"><button class="chip" data-act="imageReset">Reset adjustments</button></div>
      </div>`;
    }

    if (!(s.callout && s.callout.kind === 'icon')) {
      html += `<div class="insp-section">
        <div class="insp-title">Text</div>
        ${s.callout ? '' : `<div class="insp-row"><textarea data-act="label" rows="3" style="width:100%;background:#1e222a;color:#dfe4ec;border:1px solid #3d4450;border-radius:4px;font:inherit;padding:4px;resize:vertical">${escapeHtml(s.label || '')}</textarea></div>`}
        <div class="insp-row">
          <input type="number" data-act="fontSize" value="${s.fontSize}" title="Font size"/>
          <button class="chip ${s.bold ? 'active' : ''}" data-act="bold"><b>B</b></button>
          <button class="chip ${s.italic ? 'active' : ''}" data-act="italic"><i>I</i></button>
          <button class="chip ${s.underline ? 'active' : ''}" data-act="underline"><u>U</u></button>
          <input type="color" data-act="textColor" value="${s.textColor}" title="Text colour"/>
        </div>
        <div class="insp-row">
          <select data-act="align">
            <option value="left" ${s.align === 'left' ? 'selected' : ''}>Left</option>
            <option value="center" ${s.align === 'center' ? 'selected' : ''}>Center</option>
            <option value="right" ${s.align === 'right' ? 'selected' : ''}>Right</option>
          </select>
          <label style="flex:0 0 auto"><input type="checkbox" data-act="textFit" ${s.textFit && (s.textFit.width || s.textFit.height) ? 'checked' : ''}/> Fit text</label>
        </div>
      </div>`;
    }

    // Per-run formatting and field insertion.
    if (s.rich && !s.callout) {
      const fieldOpts = [
        ['PAGENUMBER()', 'Page number'],
        ['PAGECOUNT()', 'Page count'],
        ['FILENAME()', 'File name'],
        ['FORMAT(NOW(),"yyyy-mm-dd")', 'Date'],
        ...(s.data || []).map((d) => [`Prop.${d.name}`, d.label || d.name]),
      ];
      html += `<div class="insp-section"><div class="insp-title">Text runs</div>`;
      s.rich.paragraphs.forEach((p, pi) => {
        html += `<div class="rich-para">
          <div class="insp-row">
            <label>P${pi + 1}</label>
            <select data-act="richParaAlign" data-para="${pi}">
              ${['left', 'center', 'right'].map((a) => `<option value="${a}" ${p.align === a ? 'selected' : ''}>${a}</option>`).join('')}
            </select>
            <button class="chip ${p.bullet ? 'active' : ''}" data-act="richParaBullet" data-para="${pi}" title="Bullet">•</button>
            <button class="chip" data-act="richRunAdd" data-para="${pi}" title="Add run">+ Run</button>
          </div>`;
        (p.runs || []).forEach((r, ri) => {
          const st = r.style || {};
          const styleChips = `
            <input type="number" data-act="richRunSize" data-para="${pi}" data-run="${ri}" value="${st.fontSize || s.fontSize}" title="Size" style="width:52px"/>
            <button class="chip ${st.bold ? 'active' : ''}" data-act="richRunBold" data-para="${pi}" data-run="${ri}"><b>B</b></button>
            <button class="chip ${st.italic ? 'active' : ''}" data-act="richRunItalic" data-para="${pi}" data-run="${ri}"><i>I</i></button>
            <button class="chip ${st.underline ? 'active' : ''}" data-act="richRunUnderline" data-para="${pi}" data-run="${ri}"><u>U</u></button>
            <input type="color" data-act="richRunColor" data-para="${pi}" data-run="${ri}" value="${st.textColor || s.textColor}" title="Colour"/>
            <button class="chip" data-act="richRunRemove" data-para="${pi}" data-run="${ri}" title="Remove run">×</button>`;
          if (r.field) {
            html += `<div class="insp-row">
              <input type="text" data-act="richFieldFormula" data-para="${pi}" data-run="${ri}" value="${escapeHtml(r.field.formula || '')}" title="Field formula"/>
              <span title="Cached value">${escapeHtml(r.field.value || '')}</span>
              ${styleChips}
            </div>`;
          } else {
            html += `<div class="insp-row">
              <input type="text" data-act="richRunText" data-para="${pi}" data-run="${ri}" value="${escapeHtml(r.text || '')}" title="Run text"/>
              ${styleChips}
            </div>`;
          }
        });
        html += `</div>`;
      });
      html += `<div class="insp-row">
        <select data-act="richField">
          <option value="">Insert field…</option>
          ${fieldOpts.map(([f, l]) => `<option value="${escapeHtml(f)}">${escapeHtml(l)}</option>`).join('')}
        </select>
        <button class="chip" data-act="richParaAdd">Add paragraph</button>
      </div></div>`;
    } else if (!s.callout) {
      html += `<div class="insp-section"><div class="insp-title">Rich text</div>
        <div class="insp-row"><button class="chip" data-act="makeRich">Convert to rich text</button></div>
      </div>`;
    }

    // Shape Data (formerly custom properties).
    if (!s.callout) {
      html += `<div class="insp-section"><div class="insp-title">Shape Data</div>`;
      (s.data || []).forEach((d, i) => {
        html += `<div class="insp-row">
          <input type="text" data-act="dataLabel" data-index="${i}" value="${escapeHtml(d.label || '')}" title="Label" style="width:42%"/>
          <input type="text" data-act="dataValue" data-index="${i}" value="${escapeHtml(d.value || '')}" title="Value" style="width:42%"/>
          <span class="muted" title="Property name">${escapeHtml(d.name)}</span>
          <button class="chip" data-act="dataRemove" data-index="${i}" title="Remove">×</button>
        </div>`;
      });
      html += `<div class="insp-row"><button class="chip" data-act="dataAdd">Add property</button></div></div>`;
    }

    if (!s.callout && s.dataColor) {
      const rows = s.data || [];
      const color = dataColorFill(s);
      html += `<div class="insp-section">
        <div class="insp-title">Color by value</div>
        <div class="insp-row"><label>Field</label>
          <select data-act="dataColorField">
            ${rows.map((d) => `<option value="${escapeHtml(d.name)}" ${String(d.name) === String(s.dataColor.field) ? 'selected' : ''}>${escapeHtml(d.label || d.name)}</option>`).join('')}
            ${rows.some((d) => String(d.name) === String(s.dataColor.field)) ? '' : `<option value="${escapeHtml(s.dataColor.field)}" selected>${escapeHtml(s.dataColor.field)}</option>`}
          </select></div>
        <div class="insp-row"><label>Min / Max</label>
          <input type="number" data-act="dataColorMin" value="${Number.isFinite(Number(s.dataColor.min)) ? s.dataColor.min : 0}"/>
          <input type="number" data-act="dataColorMax" value="${Number.isFinite(Number(s.dataColor.max)) ? s.dataColor.max : 100}"/></div>
        <div class="insp-row"><label>Low / Mid / High</label>
          <input type="color" data-act="dataColorLow" value="${escapeHtml(s.dataColor.low || '#f8696b')}" title="Low band"/>
          <input type="color" data-act="dataColorMid" value="${escapeHtml(s.dataColor.mid || '#ffeb84')}" title="Middle band"/>
          <input type="color" data-act="dataColorHigh" value="${escapeHtml(s.dataColor.high || '#63be7b')}" title="High band"/>
        </div>
        <div class="insp-row"><span class="muted">Equal thirds. Current fill: ${color ? `<span class="color-chip" style="background:${color}"></span> ${escapeHtml(color)}` : 'not numeric'}.</span></div>
        <div class="insp-row"><button class="chip" data-act="removeDataColor">Remove colour rule</button></div>
      </div>`;
    }

    if (!s.callout) {
      const rows = s.data || [];
      const field = rows.some((d) => String(d.name) === String(app.state.dgField))
        ? app.state.dgField : (rows[0]?.name || '');
      const callouts = app.calloutsFor ? app.calloutsFor([s.id]) : [];
      html += `<div class="insp-section">
        <div class="insp-title">Data graphics</div>`;
      if (rows.length) {
        html += `<div class="insp-row"><label>Field</label>
          <select data-act="dgField">
            ${rows.map((d) => `<option value="${escapeHtml(d.name)}" ${String(d.name) === String(field) ? 'selected' : ''}>${escapeHtml(d.label || d.name)}</option>`).join('')}
          </select></div>
        <div class="insp-row"><label>Callout</label>
          <button class="chip" data-act="addCallout" data-val="top" title="Add a callout above">↑</button>
          <button class="chip" data-act="addCallout" data-val="bottom" title="Add a callout below">↓</button>
          <button class="chip" data-act="addCallout" data-val="left" title="Add a callout on the left">←</button>
          <button class="chip" data-act="addCallout" data-val="right" title="Add a callout on the right">→</button>
        </div>
        <div class="insp-row"><label>Heading</label>
          <button class="chip" data-act="addHeading" title="Add a filled heading above">Add heading</button>
        </div>
        <div class="insp-row"><label>Bar</label>
          <button class="chip" data-act="addBar" data-val="top" title="Add a data bar above">↑</button>
          <button class="chip" data-act="addBar" data-val="bottom" title="Add a data bar below">↓</button>
          <button class="chip" data-act="addBar" data-val="left" title="Add a data bar on the left">←</button>
          <button class="chip" data-act="addBar" data-val="right" title="Add a data bar on the right">→</button>
        </div>
        <div class="insp-row"><label>Icon</label>
          <button class="chip" data-act="addIcon" data-val="top" title="Add a data icon above">↑</button>
          <button class="chip" data-act="addIcon" data-val="bottom" title="Add a data icon below">↓</button>
          <button class="chip" data-act="addIcon" data-val="left" title="Add a data icon on the left">←</button>
          <button class="chip" data-act="addIcon" data-val="right" title="Add a data icon on the right">→</button>
        </div>
        <div class="insp-row"><label>Colour</label>
          ${s.dataColor
            ? `<span class="muted">Color rule active</span>`
            : `<button class="chip" data-act="addDataColor">Color by value</button>`}
        </div>`;
      } else {
        html += `<div class="insp-row"><span class="muted">Add Shape Data above to create a data callout, bar or icon.</span></div>`;
      }
      for (const c of callouts) {
        const kind = c.callout.kind === 'bar' ? 'Bar'
          : c.callout.kind === 'icon' ? 'Icon'
            : c.callout.kind === 'heading' ? 'Heading' : 'Callout';
        html += `<div class="insp-row">
          <button class="chip" data-act="editCallout" data-callout="${c.id}" title="Edit this data graphic">${kind} · ${escapeHtml(c.callout.field)} · ${escapeHtml(c.callout.position || (c.callout.kind === 'icon' ? 'left' : 'top'))}</button>
          <button class="chip" data-act="removeCallout" data-callout="${c.id}" title="Remove">×</button>
        </div>`;
      }
      html += `</div>`;
    }

    html += `<div class="insp-section"><div class="insp-title">Hyperlinks</div>`;
    (s.hyperlinks || []).forEach((h, i) => {
      html += `<div class="insp-row">
        <input type="text" data-act="linkAddress" data-index="${i}" value="${escapeHtml(h.address || '')}" placeholder="Address" title="Address" style="width:46%"/>
        <input type="text" data-act="linkSubAddress" data-index="${i}" value="${escapeHtml(h.subAddress || '')}" placeholder="Sub-address" title="Sub-address" style="width:46%"/>
        <button class="chip" data-act="linkRemove" data-index="${i}" title="Remove">×</button>
      </div>`;
    });
    html += `<div class="insp-row"><button class="chip" data-act="linkAdd">Add hyperlink</button></div></div>`;
  }

  if (conns.length === 1 && !shapes.length) {
    const c = conns[0];
    html += `<div class="insp-section">
      <div class="insp-title">Connector</div>
      <div class="insp-row"><label>Route</label>
        <select data-act="route">
          <option value="straight" ${c.route === 'straight' ? 'selected' : ''}>Straight</option>
          <option value="elbow" ${c.route === 'elbow' ? 'selected' : ''}>Right angle</option>
          <option value="curved" ${c.route === 'curved' ? 'selected' : ''}>Curved</option>
        </select></div>
      <div class="insp-row"><label>Visio</label>
        <select data-act="routeStyle">
          <option value="">Default</option>
          ${[0, 1, 2, 3, 4, 5, 6, 7, 8].map((v) => `<option value="${v}" ${c.routeStyle === v ? 'selected' : ''}>${({
            0: 'Auto (0)', 1: 'Right angle (1)', 2: 'Straight (2)',
            3: 'Right angle 3', 4: 'Right angle 4', 5: 'Right angle 5',
            6: 'Right angle 6', 7: 'Straight 7', 8: 'Straight 8',
          })[v]}</option>`).join('')}
        </select></div>
      <div class="insp-row"><label>Fixed</label>
        <select data-act="conFixedCode">
          <option value="">Auto</option>
          ${[0, 1, 2, 3, 4, 5, 6].map((v) => `<option value="${v}" ${c.conFixedCode === v ? 'selected' : ''}>${v}</option>`).join('')}
        </select>
        <label style="flex:0 0 auto">Ext</label>
        <select data-act="lineRouteExt">
          <option value="">Auto</option>
          ${[0, 1, 2].map((v) => `<option value="${v}" ${c.lineRouteExt === v ? 'selected' : ''}>${v}</option>`).join('')}
        </select></div>
      <div class="insp-row"><label>Walk</label>
        <select data-act="walkPreference">
          <option value="">Auto</option>
          ${[0, 1, 2, 3].map((v) => `<option value="${v}" ${c.walkPreference === v ? 'selected' : ''}>${v}</option>`).join('')}
        </select>
        <label style="flex:0 0 auto">Jumps</label>
        <select data-act="plowCode">
          <option value="">Auto</option>
          ${[0, 1, 2, 3].map((v) => `<option value="${v}" ${c.plowCode === v ? 'selected' : ''}>${v}</option>`).join('')}
        </select></div>
      <div class="insp-row"><label>Label</label>
        <input type="text" data-act="connLabel" value="${escapeHtml(c.label || '')}"/></div>
      <div class="insp-row"><label>Start</label>
        <select data-act="arrowStart">
          ${arrowOpts(c.arrowStart)}
        </select>
        <label style="flex:0 0 auto">End</label>
        <select data-act="arrowEnd">
          ${arrowOpts(c.arrowEnd)}
        </select>
      </div>
    </div>`;
  }

  // Group membership is shared by every selected member, so report it once.
  const grouped = [...shapes, ...conns].find((o) => o.group);
  if (grouped) {
    const pg = app.getPage();
    const root = String(grouped.group.rootId || grouped.group.id);
    const rec = (pg.groups || []).find((g) => String(g.visioId) === root);
    const members = [...pg.shapes, ...pg.connectors]
      .filter((m) => m.group && String(m.group.rootId) === root);
    const name = rec?.label || rec?.name || `Group ${root}`;
    html += `<div class="insp-section">
      <div class="insp-title">Group</div>
      <div class="insp-row"><label>Name</label><span>${escapeHtml(name)}</span></div>
      <div class="insp-row"><label>Members</label><span>${members.length}</span></div>
      <div class="insp-row"><label>Depth</label><span>${grouped.group.depth || 1}</span></div>
      ${grouped.group.rotated ? '<div class="insp-row"><label>Rotation</label><span>Rotated group</span></div>' : ''}
    </div>`;
  }

  // Swimlane / CFF controls. The context follows the selected lane, list or
  // task, so a task can be moved without first selecting the swimlane frame.
  const cff = app.swimlaneContext();
  if (cff) {
    const lanes = app.swimlaneLanes();
    const laneHeading = cff.lane
      ? (cff.lane.heading || cff.lane.label
        || app.getPage().shapes.find((s) => s.id === cff.lane.modelId)?.label || '')
      : '';
    html += `<div class="insp-section">
      <div class="insp-title">Swimlane</div>`;
    if (cff.lane) {
      html += `<div class="insp-row"><label>Lane</label>
        <input type="text" data-act="laneHeading" value="${escapeHtml(laneHeading)}"/></div>`;
    }
    html += `<div class="insp-row"><label>Lanes</label><span>${lanes.length}</span>
      <button class="chip" data-act="addLaneAbove" title="Insert a lane above">+ Above</button>
      <button class="chip" data-act="addLaneBelow" title="Insert a lane below">+ Below</button>
    </div>
    <div class="insp-grid" style="margin-top:6px">
      <button class="chip" data-act="addPhase" title="Add a phase separator">Add phase</button>
      <button class="chip" data-act="removePhase" title="Remove the selected phase">Remove phase</button>
      <button class="chip" data-act="removeLane" title="Remove the selected lane">Remove lane</button>
      <button class="chip" data-act="removeSwimlane" title="Remove the whole swimlane">Remove swimlane</button>
    </div>`;
    if (lanes.length) {
      html += `<div class="insp-row"><label>Move to</label>
        <select data-act="moveLane">
          <option value="">Lane…</option>
          ${lanes.map((l) => `<option value="${escapeHtml(l.ref)}">${escapeHtml(l.heading)}</option>`).join('')}
        </select></div>`;
    }
    html += `</div>`;
  }

  // shared formatting
  const first = shapes[0] || conns[0];
  if (first) {
    html += `<div class="insp-section">
      <div class="insp-title">Fill &amp; Line</div>`;
    if (shapes.length) {
      html += shapes.some((s) => s.dataColor)
        ? `<div class="insp-row"><span class="muted">Fill is controlled by Color by value.</span></div>`
        : `<div class="insp-row">
            <label>Fill</label>
            <input type="color" data-act="fill" value="${first.fill || '#dbe9f6'}" title="Fill colour"/>
            <button class="chip" data-act="fillNone">None</button>
          </div>`;
    }
    html += `<div class="insp-row">
        <label>Line</label>
        <input type="color" data-act="stroke" value="${first.stroke || '#3b6ea5'}" title="Line colour"/>
        <select data-act="strokeWidth" style="flex:0 0 62px">
          ${[0, 0.5, 1, 1.5, 2, 3, 5].map((w) => `<option value="${w}" ${Math.abs((first.strokeWidth ?? 1) - w) < 0.01 ? 'selected' : ''}>${w} px</option>`).join('')}
        </select>
      </div>
      <div class="insp-row">
        <label>Style</label>
        <select data-act="strokeStyle">
          <option value="solid" ${first.strokeStyle === 'solid' ? 'selected' : ''}>Solid</option>
          <option value="dashed" ${first.strokeStyle === 'dashed' ? 'selected' : ''}>Dashed</option>
          <option value="dotted" ${first.strokeStyle === 'dotted' ? 'selected' : ''}>Dotted</option>
        </select>
      </div>
    </div>`;
  }

  if (shapes.length) {
    html += `<div class="insp-section">
      <div class="insp-title">Arrange</div>
      <div class="insp-grid">
        <button class="chip" data-act="align" data-val="left" title="Align left">⇤</button>
        <button class="chip" data-act="align" data-val="hcenter" title="Align centres">⇔</button>
        <button class="chip" data-act="align" data-val="right" title="Align right">⇥</button>
        <button class="chip" data-act="align" data-val="top" title="Align top">⇧</button>
        <button class="chip" data-act="align" data-val="vcenter" title="Align middles">⇕</button>
        <button class="chip" data-act="align" data-val="bottom" title="Align bottom">⇩</button>
      </div>
      <div class="insp-grid" style="margin-top:6px">
        <button class="chip" data-act="front">Front</button>
        <button class="chip" data-act="back">Back</button>
        <button class="chip" data-act="flip-h">Flip H</button>
        <button class="chip" data-act="flip-v">Flip V</button>
        <button class="chip" data-act="rotate">Rotate 90&deg;</button>
        <button class="chip" data-act="delete" title="Delete (Del)">Delete</button>
      </div>
      <div class="insp-grid" style="margin-top:6px">
        <button class="chip" data-act="relayout" data-val="vertical" title="Layered top-down layout for the selection">Layout ↓</button>
        <button class="chip" data-act="relayout" data-val="horizontal" title="Layered left-to-right layout for the selection">Layout →</button>
      </div>
    </div>`;
  }

  html += themeSectionHTML(app);
  return html;
}

function arrowOpts(v) {
  return `
    <option value="none" ${v === 'none' ? 'selected' : ''}>None</option>
    <option value="arrow" ${v === 'arrow' ? 'selected' : ''}>Open</option>
    <option value="filled" ${v === 'filled' ? 'selected' : ''}>Filled</option>
    <option value="hollowTriangle" ${v === 'hollowTriangle' ? 'selected' : ''}>Hollow triangle</option>
    <option value="hollowDiamond" ${v === 'hollowDiamond' ? 'selected' : ''}>Hollow diamond</option>
    <option value="filledDiamond" ${v === 'filledDiamond' ? 'selected' : ''}>Filled diamond</option>`;
}

function wireInspector(app, host) {
  host.addEventListener('change', (e) => {
    const act = e.target.dataset.act;
    if (!act) return;
    const v = e.target.type === 'checkbox' ? e.target.checked : e.target.value;
    applyInspectorAction(app, act, v, e.target);
  });
  host.addEventListener('input', (e) => {
    const act = e.target.dataset.act;
    if (!act || e.target.type === 'color') return;
    if (!['label', 'connLabel', 'x', 'y', 'w', 'h', 'rot', 'fontSize', 'laneHeading'].includes(act)) return;
    applyInspectorAction(app, act, e.target.value, e.target, true);
  });
  host.addEventListener('click', (e) => {
    const b = e.target.closest('[data-act]');
    if (!b || b.tagName === 'SELECT' || b.tagName === 'INPUT' || b.tagName === 'TEXTAREA') return;
    applyInspectorAction(app, b.dataset.act, b.dataset.val, b);
  });
}

function applyInspectorAction(app, act, value, el, live = false) {
  const sel = [...app.state.selection];
  const pg = app.getPage();
  const shapes = pg.shapes.filter((s) => sel.includes(s.id));
  const conns = pg.connectors.filter((c) => sel.includes(c.id));
  const before = app.history.snapshot(app.doc);

  switch (act) {
    case 'type': shapes.forEach((s) => { s.type = value; }); break;
    case 'x': shapes.forEach((s) => { s.x = num(value); if (s.callout) s.callout.position = 'free'; }); break;
    case 'y': shapes.forEach((s) => { s.y = num(value); if (s.callout) s.callout.position = 'free'; }); break;
    case 'w': shapes.forEach((s) => { s.w = Math.max(6, num(value)); if (s.callout) s.callout.autoSize = false; }); break;
    case 'h': shapes.forEach((s) => { s.h = Math.max(6, num(value)); if (s.callout) s.callout.autoSize = false; }); break;
    case 'rot': shapes.forEach((s) => { s.rot = num(value); }); break;
    case 'label':
      shapes.forEach((s) => {
        if (s.callout) s.callout.label = value;
        else { s.label = value; delete s.rich; }
      });
      break;
    case 'connLabel': conns.forEach((c) => { c.label = value; }); break;
    case 'fontSize': shapes.forEach((s) => { s.fontSize = Math.max(4, num(value)); }); break;
    case 'align': app.alignShapes(value); break;
    case 'bold': app.toggleStyle('bold'); break;
    case 'italic': app.toggleStyle('italic'); break;
    case 'underline': app.toggleStyle('underline'); break;
    case 'textAlign': shapes.forEach((s) => { s.align = value; }); break;
    case 'fill': shapes.forEach((s) => { s.fill = value; }); break;
    case 'fillNone': shapes.forEach((s) => { s.fill = 'none'; }); break;
    case 'stroke': shapes.concat(conns).forEach((o) => { o.stroke = value; }); break;
    case 'strokeWidth': shapes.concat(conns).forEach((o) => { o.strokeWidth = num(value); }); break;
    case 'strokeStyle': shapes.concat(conns).forEach((o) => { o.strokeStyle = value; }); break;
    case 'route': conns.forEach((c) => {
      c.route = value;
      c.waypoints = [];
      if (value === 'straight') c.routeStyle = 2;
      else if (value === 'elbow') c.routeStyle = 1;
      else { delete c.routeStyle; c.conFixedCode = 3; }
    }); break;
    case 'routeStyle': conns.forEach((c) => {
      if (value === '') {
        delete c.routeStyle;
      } else {
        const n = Number(value);
        c.routeStyle = n;
        c.route = [2, 7, 8].includes(n) ? 'straight' : 'elbow';
        c.waypoints = [];
      }
    }); break;
    case 'conFixedCode':
      conns.forEach((c) => { if (value === '') delete c.conFixedCode; else c.conFixedCode = Number(value); });
      break;
    case 'lineRouteExt':
      conns.forEach((c) => { if (value === '') delete c.lineRouteExt; else c.lineRouteExt = Number(value); });
      break;
    case 'walkPreference':
      conns.forEach((c) => { if (value === '') delete c.walkPreference; else c.walkPreference = Number(value); });
      break;
    case 'plowCode':
      conns.forEach((c) => { if (value === '') delete c.plowCode; else c.plowCode = Number(value); });
      break;
    case 'arrowStart': conns.forEach((c) => { c.arrowStart = value; }); break;
    case 'arrowEnd': conns.forEach((c) => { c.arrowEnd = value; }); break;
    case 'front': app.reorder(1); break;
    case 'back': app.reorder(-1); break;
    case 'relayout':
      app.relayout({ direction: value === 'horizontal' ? 'horizontal' : 'vertical' });
      return;
    case 'flip-h': shapes.forEach((s) => { s.flipX = !s.flipX; }); break;
    case 'flip-v': shapes.forEach((s) => { s.flipY = !s.flipY; }); break;
    case 'rotate': shapes.forEach((s) => { s.rot = (s.rot || 0) + 90; }); break;
    case 'delete': app.deleteSelection(); return;
    case 'laneHeading': app.setLaneHeading(value, live); return;
    case 'addLaneAbove': app.addLane('above'); return;
    case 'addLaneBelow': app.addLane('below'); return;
    case 'removeLane': app.removeLane(); return;
    case 'addPhase': app.addPhase(); return;
    case 'removePhase': app.removePhase(); return;
    case 'removeSwimlane': app.removeSwimlane(); return;
    case 'moveLane': if (value) app.moveSelectionToLane(value); return;
    case 'makeRich': shapes.forEach((s) => app.ensureRich(s)); break;
    case 'textFit':
      shapes.forEach((s) => {
        if (value) s.textFit = { width: true, height: true };
        else delete s.textFit;
      });
      break;
    case 'richParaAlign':
      shapes.forEach((s) => {
        const p = s.rich?.paragraphs?.[Number(el.dataset.para)];
        if (p) p.align = value;
      });
      break;
    case 'richParaBullet':
      shapes.forEach((s) => {
        const p = s.rich?.paragraphs?.[Number(el.dataset.para)];
        if (p) p.bullet = !p.bullet;
      });
      break;
    case 'richParaAdd':
      shapes.forEach((s) => {
        const rich = app.ensureRich(s);
        rich.paragraphs.push({ align: s.align || 'center', bullet: false, bulletStr: '•', indent: 0, runs: [] });
      });
      break;
    case 'richRunAdd':
      shapes.forEach((s) => {
        const p = s.rich?.paragraphs?.[Number(el.dataset.para)];
        if (p) p.runs.push({ text: '', style: { fontFamily: s.fontFamily, fontSize: s.fontSize, bold: !!s.bold, italic: !!s.italic, underline: !!s.underline, textColor: s.textColor } });
      });
      break;
    case 'richRunRemove':
      shapes.forEach((s) => {
        const p = s.rich?.paragraphs?.[Number(el.dataset.para)];
        if (p) p.runs.splice(Number(el.dataset.run), 1);
        app.syncRichLabel(s);
      });
      break;
    case 'richRunText':
      shapes.forEach((s) => {
        const run = s.rich?.paragraphs?.[Number(el.dataset.para)]?.runs?.[Number(el.dataset.run)];
        if (run) run.text = value;
        app.syncRichLabel(s);
      });
      break;
    case 'richRunSize':
    case 'richRunBold':
    case 'richRunItalic':
    case 'richRunUnderline':
    case 'richRunColor':
      shapes.forEach((s) => {
        const run = s.rich?.paragraphs?.[Number(el.dataset.para)]?.runs?.[Number(el.dataset.run)];
        if (!run) return;
        run.style = run.style || {};
        if (act === 'richRunSize') run.style.fontSize = Math.max(4, num(value));
        if (act === 'richRunBold') run.style.bold = !run.style.bold;
        if (act === 'richRunItalic') run.style.italic = !run.style.italic;
        if (act === 'richRunUnderline') run.style.underline = !run.style.underline;
        if (act === 'richRunColor') run.style.textColor = value;
      });
      break;
    case 'richFieldFormula':
      shapes.forEach((s) => {
        const run = s.rich?.paragraphs?.[Number(el.dataset.para)]?.runs?.[Number(el.dataset.run)];
        if (!run || !run.field) return;
        run.field.formula = value;
        run.field.value = app.fieldValueFor(s, value);
        run.text = run.field.value;
        app.syncRichLabel(s);
      });
      break;
    case 'richField':
      if (value) shapes.forEach((s) => app.insertField(s, value));
      break;
    case 'dataAdd':
      shapes.forEach((s) => {
        s.data = s.data || [];
        let n = s.data.length + 1;
        while (s.data.some((d) => String(d.name) === `Row_${n}`)) n++;
        s.data.push({ name: `Row_${n}`, label: `Property ${n}`, value: '', prompt: '', format: '', type: 0, invisible: 0 });
      });
      break;
    case 'dataRemove':
      shapes.forEach((s) => {
        const row = s.data?.[Number(el.dataset.index)];
        if (!row) return;
        s.data.splice(Number(el.dataset.index), 1);
        const prop = `Prop.${row.name}`;
        if (s.rich) {
          for (const p of s.rich.paragraphs) {
            p.runs = p.runs.filter((r) => !(r.field && r.field.formula === prop));
          }
          app.syncRichLabel(s);
        }
      });
      break;
    case 'dataLabel':
      shapes.forEach((s) => {
        const row = s.data?.[Number(el.dataset.index)];
        if (row) row.label = value;
      });
      break;
    case 'dataValue':
      shapes.forEach((s) => {
        const row = s.data?.[Number(el.dataset.index)];
        if (!row) return;
        row.value = value;
        const prop = `Prop.${row.name}`;
        if (s.rich) {
          for (const p of s.rich.paragraphs) {
            for (const r of p.runs) {
              if (r.field && r.field.formula === prop) {
                r.field.value = value;
                r.text = value;
              }
            }
          }
          app.syncRichLabel(s);
        }
      });
      break;
    case 'linkAdd':
      shapes.forEach((s) => {
        s.hyperlinks = s.hyperlinks || [];
        let n = s.hyperlinks.length + 1;
        while (s.hyperlinks.some((h) => String(h.name) === `Link_${n}`)) n++;
        s.hyperlinks.push({
          name: `Link_${n}`, address: '', subAddress: '', description: '',
          extraInfo: '', frame: '', newWindow: 0, default: 0, invisible: 0, sortKey: '',
        });
      });
      break;
    case 'linkRemove':
      shapes.forEach((s) => {
        if (s.hyperlinks) s.hyperlinks.splice(Number(el.dataset.index), 1);
      });
      break;
    case 'linkAddress':
      shapes.forEach((s) => {
        const h = s.hyperlinks?.[Number(el.dataset.index)];
        if (h) h.address = value;
      });
      break;
    case 'linkSubAddress':
      shapes.forEach((s) => {
        const h = s.hyperlinks?.[Number(el.dataset.index)];
        if (h) h.subAddress = value;
      });
      break;
    case 'imageReplace':
      if (shapes[0]?.image) app.imagePick(shapes[0].id);
      return;
    case 'imageFit':
      shapes.forEach((s) => { if (s.image) app.fitImageNatural(s); });
      return;
    case 'imageFrame':
      shapes.forEach((s) => { if (s.image) app.fitImageFrame(s); });
      return;
    case 'imageReset':
      shapes.forEach((s) => { if (s.image) app.resetImageAdjustments(s); });
      return;
    case 'imageBrightness':
      shapes.forEach((s) => { if (s.image) s.image.brightness = clamp(num(value) / 100, -1, 1); });
      break;
    case 'imageContrast':
      shapes.forEach((s) => { if (s.image) s.image.contrast = clamp(num(value) / 100, -1, 1); });
      break;
    case 'imageGamma':
      shapes.forEach((s) => { if (s.image) s.image.gamma = clamp(num(value), 0.1, 3); });
      break;
    case 'imageTransparency':
      shapes.forEach((s) => { if (s.image) s.image.transparency = clamp(num(value) / 100, 0, 1); });
      break;
    case 'cropLeft':
    case 'cropTop':
    case 'cropRight':
    case 'cropBottom': {
      const side = act.slice(4).toLowerCase();
      shapes.forEach((s) => {
        if (s.image && app.setImageCrop) app.setImageCrop(s.image, side, num(value) / 100);
      });
      break;
    }
    case 'dgField':
      app.state.dgField = value;
      app.updateInspector();
      return;
    case 'addCallout':
      shapes.forEach((s) => app.addDataCallout(s.id, app.state.dgField || (s.data?.[0]?.name), value));
      return;
    case 'addHeading':
      shapes.forEach((s) => app.addDataHeading(s.id, app.state.dgField || (s.data?.[0]?.name), 'top'));
      return;
    case 'addBar':
      shapes.forEach((s) => app.addDataBar(s.id, app.state.dgField || (s.data?.[0]?.name), value));
      return;
    case 'addIcon':
      shapes.forEach((s) => app.addDataIcon(s.id, app.state.dgField || (s.data?.[0]?.name), value));
      return;
    case 'addDataColor':
      shapes.forEach((s) => app.addDataColor(s.id, app.state.dgField || (s.data?.[0]?.name)));
      return;
    case 'removeDataColor':
      shapes.forEach((s) => app.removeDataColor(s.id));
      return;
    case 'dataColorField':
      shapes.forEach((s) => { if (s.dataColor) s.dataColor.field = value; });
      break;
    case 'dataColorMin':
      shapes.forEach((s) => { if (s.dataColor) s.dataColor.min = num(value); });
      break;
    case 'dataColorMax':
      shapes.forEach((s) => { if (s.dataColor) s.dataColor.max = num(value); });
      break;
    case 'dataColorLow':
      shapes.forEach((s) => { if (s.dataColor) s.dataColor.low = value; });
      break;
    case 'dataColorMid':
      shapes.forEach((s) => { if (s.dataColor) s.dataColor.mid = value; });
      break;
    case 'dataColorHigh':
      shapes.forEach((s) => { if (s.dataColor) s.dataColor.high = value; });
      break;
    case 'editCallout':
      if (el.dataset.callout) app.setSelection([el.dataset.callout]);
      return;
    case 'removeCallout': {
      const id = el.dataset.callout || shapes[0]?.id;
      if (id) app.removeDataCallout(id);
      return;
    }
    case 'calloutField':
      shapes.forEach((s) => { if (s.callout) s.callout.field = value; });
      break;
    case 'calloutLabel':
      shapes.forEach((s) => { if (s.callout) s.callout.label = value; });
      break;
    case 'iconSet':
      shapes.forEach((s) => {
        if (s.callout && s.callout.kind === 'icon') s.callout.iconSet = value;
      });
      break;
    case 'calloutPosition':
      shapes.forEach((s) => { if (s.callout) s.callout.position = value; });
      break;
    case 'calloutAuto':
      shapes.forEach((s) => { if (s.callout) s.callout.autoSize = !!value; });
      break;
    case 'barMin':
      shapes.forEach((s) => { if (s.callout) s.callout.min = num(value); });
      break;
    case 'barMax':
      shapes.forEach((s) => { if (s.callout) s.callout.max = num(value); });
      break;
    case 'barShowValue':
      shapes.forEach((s) => { if (s.callout) s.callout.showValue = !!value; });
      break;
    case 'themePreset':
      if (value && value !== '__current') app.applyThemePreset(value);
      return;
    case 'themeVariation':
      app.setThemeVariation(value);
      return;
    case 'themeScope':
      app.state.themeScope = value;
      app.updateInspector();
      return;
    case 'quickStyle':
      app.applyQuickStyle(value);
      return;
    default: return;
  }

  if (!live) {
    if (before.json !== app.history.snapshot(app.doc).json) app.history.pushSnapshot(before);
  }
  app.render();
  app.updateInspector();
}

function patchInspector(app, host, shapes, conns) {
  if (!shapes.length) return;
  const s = shapes[0];
  const set = (act, v) => {
    const el = host.querySelector(`[data-act="${act}"]`);
    if (el && document.activeElement !== el && el.value !== String(v)) el.value = v;
  };
  set('x', round(s.x)); set('y', round(s.y)); set('w', round(s.w)); set('h', round(s.h));
  set('rot', round(s.rot || 0));
}

// ------------------------------------------------------------ page tabs --
export function buildPageTabs(app) {
  const host = $('#pageTabs');
  host.innerHTML = '';
  app.doc.pages.forEach((p, i) => {
    const tab = document.createElement('div');
    tab.className = `page-tab ${i === app.doc.pageIndex ? 'active' : ''}`;
    tab.innerHTML = `<span class="pname">${escapeHtml(p.name)}</span>` +
      (app.doc.pages.length > 1 ? '<span class="close" data-close="' + p.id + '">&times;</span>' : '');
    tab.addEventListener('click', (e) => {
      if (e.target.dataset.close) return;
      app.setPage(i);
    });
    tab.querySelector('.pname').addEventListener('dblclick', () => {
      const name = prompt('Page name', p.name);
      if (name) { app.history.push(app.doc); p.name = name; app.render(); buildPageTabs(app); }
    });
    const close = tab.querySelector('.close');
    if (close) close.addEventListener('click', (e) => { e.stopPropagation(); app.deletePage(p.id); });
    host.appendChild(tab);
  });
  const add = document.createElement('div');
  add.className = 'page-add';
  add.textContent = '+';
  add.title = 'Add page';
  add.addEventListener('click', () => app.addPage());
  host.appendChild(add);
}

// --------------------------------------------------------- context menu --
export function initContextMenu(app) {
  const menu = $('#ctxMenu');
  const stage = $('#stage');
  let open = false;

  const close = () => {
    menu.classList.remove('show');
    open = false;
  };

  const item = (label, key, fn, opts = {}) => {
    const el = document.createElement('div');
    el.className = `ctx-item${opts.disabled ? ' disabled' : ''}${opts.active ? ' active' : ''}`;
    el.innerHTML = `<span>${label}</span>${key ? `<span class="ctx-key">${key}</span>` : ''}`;
    if (!opts.disabled) el.addEventListener('click', () => { close(); fn(); });
    return el;
  };
  const sep = () => {
    const el = document.createElement('div');
    el.className = 'ctx-sep';
    return el;
  };

  app.svg.addEventListener('contextmenu', (e) => {
    e.preventDefault();
    const target = e.target.closest('[data-shape-id],[data-conn-id]');
    const id = target?.dataset.shapeId || target?.dataset.connId;
    if (id && !app.state.selection.has(id)) app.setSelection([id]);
    if (!id && !e.shiftKey) app.setSelection([]);

    const pg = app.getPage();
    const sel = [...app.state.selection];
    const conns = pg.connectors.filter((c) => sel.includes(c.id));
    const shapes = pg.shapes.filter((s) => sel.includes(s.id));
    const has = sel.length > 0;
    const oneConn = conns.length === 1 && !shapes.length;

    menu.innerHTML = '';
    menu.appendChild(item('Cut', 'Ctrl+X', () => app.cut(), { disabled: !has }));
    menu.appendChild(item('Copy', 'Ctrl+C', () => app.copy(), { disabled: !has }));
    menu.appendChild(item('Paste', 'Ctrl+V', () => app.paste(), { disabled: !app.clipboard }));
    menu.appendChild(item('Duplicate', 'Ctrl+D', () => app.duplicate(), { disabled: !has }));
    menu.appendChild(sep());
    menu.appendChild(item('Group', 'Ctrl+G', () => app.groupSelection(), { disabled: shapes.length + conns.length < 2 }));
    menu.appendChild(item('Ungroup', 'Ctrl+Shift+G', () => app.ungroupSelection(), {
      disabled: ![...shapes, ...conns].some((o) => o.group),
    }));
    menu.appendChild(item('Create container', '', () => app.createContainer(), { disabled: !has }));
    menu.appendChild(item('Create swimlane', 'Ctrl+Shift+L', () => app.createSwimlane(), { disabled: false }));
    menu.appendChild(item('Insert picture…', '', () => app.imagePick()));
    if (shapes.length === 1 && (shapes[0].data || []).length) {
      const s = shapes[0];
      menu.appendChild(item('Add data callout', '', () =>
        app.addDataCallout(s.id, app.state.dgField || s.data[0].name, 'top')));
      menu.appendChild(item('Add data bar', '', () =>
        app.addDataBar(s.id, app.state.dgField || s.data[0].name, 'top')));
      menu.appendChild(item('Add data icon', '', () =>
        app.addDataIcon(s.id, app.state.dgField || s.data[0].name, 'left')));
      menu.appendChild(item('Add data heading', '', () =>
        app.addDataHeading(s.id, app.state.dgField || s.data[0].name, 'top')));
      if (s.dataColor) {
        menu.appendChild(item('Remove colour rule', '', () => app.removeDataColor(s.id)));
      } else {
        menu.appendChild(item('Color by value', '', () =>
          app.addDataColor(s.id, app.state.dgField || s.data[0].name)));
      }
    }
    if (shapes.length === 1 && shapes[0].callout) {
      menu.appendChild(item('Remove data graphic', '', () => app.removeDataCallout(shapes[0].id)));
    }
    menu.appendChild(sep());
    menu.appendChild(item('Re-layout: top-down', '', () => app.relayout({ direction: 'vertical' })));
    menu.appendChild(item('Re-layout: left-to-right', '', () => app.relayout({ direction: 'horizontal' })));
    // CFF internal frames have dedicated lane/phase actions; keep them out of
    // the generic container membership list.
    const CFF_ROLES = new Set(['pool', 'list', 'phase-list', 'lane', 'separator']);
    const containers = (pg.containers || []).filter((c) => !CFF_ROLES.has(c.role));
    const selectedObjs = [...shapes, ...conns];
    const cff = app.swimlaneContext();
    if (cff) {
      menu.appendChild(sep());
      if (cff.lane) {
        menu.appendChild(item('Add lane above', '', () => app.addLane('above')));
        menu.appendChild(item('Add lane below', '', () => app.addLane('below')));
        menu.appendChild(item('Remove lane', '', () => app.removeLane()));
      } else {
        menu.appendChild(item('Add lane', '', () => app.addLane('below')));
      }
      menu.appendChild(item('Add phase', '', () => app.addPhase()));
      if (cff.rec && cff.rec.role === 'separator') {
        menu.appendChild(item('Remove phase', '', () => app.removePhase()));
      }
      menu.appendChild(item('Remove swimlane', '', () => app.removeSwimlane()));
      const lanes = app.swimlaneLanes();
      const tasks = selectedObjs.filter((o) => !app.isCffFrame(o) && o.x !== undefined);
      if (lanes.length && tasks.length) {
        menu.appendChild(sep());
        for (const l of lanes.slice(0, 6)) {
          menu.appendChild(item(`Move to ${l.heading}`, '', () => app.moveSelectionToLane(l.ref)));
        }
      }
    }
    if (containers.length && selectedObjs.length) {
      const refOf = (c) => String(c.visioId || c.modelId);
      const labelOf = (c) => c.label || `${c.kind === 'list' ? 'list' : 'container'} ${refOf(c)}`;
      const addable = containers.filter((c) => {
        const ref = refOf(c);
        return selectedObjs.some((o) =>
          !(o.containers || []).map(String).includes(ref)
          && !(o.group && String(o.group.id) === ref)
          && !(o.visio && String(o.visio.id) === ref)
          && !(o.containerId && String(o.containerId) === ref));
      });
      const removable = containers.filter((c) => {
        const ref = refOf(c);
        return selectedObjs.some((o) => (o.containers || []).map(String).includes(ref));
      });
      if (addable.length || removable.length) menu.appendChild(sep());
      for (const c of addable.slice(0, 8)) {
        menu.appendChild(item(`Add to ${labelOf(c)}`, '', () => app.setContainerMembership(sel, refOf(c), true)));
      }
      for (const c of removable.slice(0, 8)) {
        menu.appendChild(item(`Remove from ${labelOf(c)}`, '', () => app.setContainerMembership(sel, refOf(c), false)));
      }
    }
    menu.appendChild(sep());
    menu.appendChild(item('Bring to Front', '', () => app.reorder(1), { disabled: !shapes.length }));
    menu.appendChild(item('Send to Back', '', () => app.reorder(-1), { disabled: !shapes.length }));
    if (oneConn) {
      menu.appendChild(sep());
      menu.appendChild(item('Route: Straight', '', () => app.setRoute('straight'), { active: conns[0].route === 'straight' }));
      menu.appendChild(item('Route: Right Angle', '', () => app.setRoute('elbow'), { active: conns[0].route === 'elbow' }));
      menu.appendChild(item('Route: Curved', '', () => app.setRoute('curved'), { active: conns[0].route === 'curved' }));
    }
    menu.appendChild(sep());
    menu.appendChild(item('Delete', 'Del', () => app.deleteSelection(), { disabled: !has }));
    menu.appendChild(item('Select All', 'Ctrl+A', () => app.selectAll()));

    const r = stage.getBoundingClientRect();
    menu.classList.add('show');
    open = true;
    const mw = menu.offsetWidth, mh = menu.offsetHeight;
    menu.style.left = `${Math.min(e.clientX - r.left, r.width - mw - 6)}px`;
    menu.style.top = `${Math.min(e.clientY - r.top, r.height - mh - 6)}px`;
  });

  document.addEventListener('pointerdown', (e) => {
    if (open && !menu.contains(e.target)) close();
  });
  window.addEventListener('blur', close);
  app.svg.addEventListener('keydown', (e) => { if (e.key === 'Escape') close(); });
}

// --------------------------------------------------------------- helpers --
export function wireToolbar(app) {
  document.addEventListener('click', (e) => {
    const btn = e.target.closest('[data-cmd]');
    if (btn) {
      const cmd = btn.dataset.cmd;
      app.runCommand(cmd);
      return;
    }
    const tool = e.target.closest('[data-tool]');
    if (tool) app.setTool(tool.dataset.tool);
  });

  const bind = (id, act, live) => {
    const el = $(id);
    if (!el) return;
    el.addEventListener(live ? 'input' : 'change', (e) => {
      app.applyStyleToSelection({ [act]: e.target.value });
    });
  };
  bind('#inFill', 'fill');
  bind('#inStroke', 'stroke');
  bind('#inTextColor', 'textColor');
  bind('#inStrokeWidth', 'strokeWidth');
  bind('#inStrokeStyle', 'strokeStyle');
  bind('#inFontSize', 'fontSize');
  bind('#inAlign', 'align');
}

export function updateToolbarState(app) {
  const sel = [...app.state.selection];
  const pg = app.getPage();
  const shapes = pg.shapes.filter((s) => sel.includes(s.id));
  const conns = pg.connectors.filter((c) => sel.includes(c.id));
  const first = shapes[0] || conns[0];
  document.querySelectorAll('[data-tool]').forEach((b) => {
    b.classList.toggle('active', b.dataset.tool === app.state.tool);
  });
  const gridBtn = document.querySelector('[data-cmd="grid"]');
  if (gridBtn) gridBtn.classList.toggle('active', app.state.grid.on);
  const snapBtn = document.querySelector('[data-cmd="snap"]');
  if (snapBtn) snapBtn.classList.toggle('active', app.state.grid.snap);
  if (first) {
    setVal('#inFill', first.fill === 'none' ? '#ffffff' : first.fill);
    setVal('#inStroke', first.stroke);
    setVal('#inTextColor', first.textColor);
    setVal('#inStrokeWidth', first.strokeWidth);
    setVal('#inStrokeStyle', first.strokeStyle);
    if (shapes[0]) {
      setVal('#inFontSize', shapes[0].fontSize);
      setVal('#inAlign', shapes[0].align);
    }
  }
  const bold = document.querySelector('[data-cmd="bold"]');
  const ital = document.querySelector('[data-cmd="italic"]');
  const und = document.querySelector('[data-cmd="underline"]');
  if (bold) bold.classList.toggle('active', !!shapes[0]?.bold);
  if (ital) ital.classList.toggle('active', !!shapes[0]?.italic);
  if (und) und.classList.toggle('active', !!shapes[0]?.underline);
}

function setVal(sel, v) {
  const el = $(sel);
  if (el && v !== undefined && v !== null) el.value = v;
}

export function updateStatus(app) {
  const sel = [...app.state.selection];
  const pg = app.getPage();
  const n = sel.length;
  const sEl = $('#statusSel');
  sEl.textContent = n === 0 ? 'No selection'
    : n === 1 ? '1 object selected'
    : `${n} objects selected`;
  $('#statusZoom').textContent = `${Math.round(app.state.view.zoom * 100)}%`;
  const hint = $('#statusHint');
  if (app.state.autosaveWarning) {
    hint.textContent = 'Autosave failed: the document or its pictures are too large for browser storage — use the VSDX button to keep a copy';
  } else if (app.state.tool === 'connector' && !app.state.connectSource) {
    hint.textContent = 'Connector: click the first shape';
  } else if (app.state.tool === 'connector') {
    hint.textContent = 'Connector: click the target shape';
  } else if (app.state.tool === 'shape') {
    hint.textContent = 'Click and drag on the canvas to draw the shape';
  } else {
    hint.textContent = `${pg.shapes.length} shapes, ${pg.connectors.length} connectors`;
  }
}

const round = (n) => Math.round(n * 10) / 10;
const num = (v) => (parseFloat(v) || 0);
const escapeHtml = (s) => String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
