// Seiten der Studie: Karten-Blätter je App, drei Gesamtlayouts, Systemtafeln, Welcome-Screens.
import { THEMES, rgba, mix, contrast, iconSvg, esc } from './kit.mjs';
import { APPS, byId, real, planned } from './apps.mjs';
import { artSvg } from './art.mjs';
import { css, page, tile, pillHtml, stateOf, cardMedia, cardTile, cardRow, controlWindow } from './ui.mjs';

const hue = (a, T) => a.hue[T.name];
const catalogApps = real;               // die acht Apps im heutigen Katalog
const installed = catalogApps.filter((a) => ['installed', 'update', 'managed'].includes(a.cardState));
const available = catalogApps.filter((a) => a.cardState === 'available');

// ---------- 1. Karten-Blatt je App ----------
export function cardSheet(app, T) {
  const body = `<div style="padding:24px;display:flex;gap:24px;align-items:flex-start;width:944px">
    <div style="width:480px;flex:none"><div class="caption t3" style="margin-bottom:8px">Media card · hero image 2:1</div>${cardMedia(app, T)}</div>
    <div style="width:320px;flex:none"><div class="caption t3" style="margin-bottom:8px">Tile card · compact grid</div>${cardTile(app, T, { width: '320px', artH: 150 })}
      <div class="caption t3" style="margin:16px 0 8px">Row · narrow and mini</div>${cardRow(app, T)}</div></div>`;
  return page(T, body);
}

// ---------- 2. Gesamtlayouts ----------
const secHead = (title, count, extra = '') => `<div class="sec-h"><span class="caption t3">${title}</span><span class="caption t3 tab">${count}</span>${extra}</div>`;
const appsHeader = (T, n, upd, short = false) => `<div class="sec-h" style="margin-bottom:16px"><span class="caption t3">${short ? 'Apps' : `Apps · ${n} installed · ${upd} updates`}</span><span class="sp"></span><span class="icon-btn">${iconSvg('refresh-cw', 16, T.text2)}</span><div class="search">${iconSvg('search', 14, T.text3)}<span>Search apps</span></div></div>`;
const grid = (cols, items, gap = 16) => `<div style="display:grid;grid-template-columns:repeat(${cols},1fr);gap:${gap}px;align-items:stretch">${items.join('')}</div>`;

export function layoutA(T, { width = 1600, cols = 3 } = {}) {
  const f = byId.calendar;
  const st = stateOf(f, 'update');
  const spotlight = `<article class="card" style="flex-direction:row;margin-bottom:24px">
    <div style="width:58%;aspect-ratio:2/1;border-right:1px solid var(--line)">${artSvg('calendar', T, { w: '100%', h: '100%' })}</div>
    <div style="flex:1;padding:32px;display:flex;flex-direction:column;gap:12px;justify-content:center">
      <span class="pill" style="align-self:flex-start"><i style="background:var(--attention)"></i>Update available</span>
      <div class="id" style="gap:16px">${tile(f, T, 48)}<div><div class="title">${f.name}</div><div class="t3">${f.q}</div></div></div>
      <p class="t2">${f.s}</p>
      <div class="meta t3 tab">${st.ver(f)}</div>
      <div style="display:flex;gap:8px;margin-top:4px"><span class="btn">Update to ${f.ver}</span><span class="btn ghost">What's new</span></div>
    </div></article>`;
  const content = `${appsHeader(T, 5, 2)}${spotlight}${secHead('Installed', 5)}${grid(cols, installed.map((a) => cardMedia(a, T)))}<div style="height:24px"></div>${secHead('Available', 3)}${grid(cols, available.map((a) => cardMedia(a, T)))}<div class="meta t3" style="margin-top:24px;display:flex;gap:8px;align-items:center">${iconSvg('check', 14, T.text3)}Checked just now · from the releases on GitHub</div>`;
  return page(T, controlWindow(T, content, { width }));
}

export function layoutB(T, { width = 1600, cols = 4, side = 'full', artH = 150, pad = 24, showPlanned = true, title } = {}) {
  const chips = ['All', 'Plan', 'Inbox', 'People', 'AI'].map((c, i) => `<span class="btn ${i ? '' : ''}" style="${i === 0 ? 'background:var(--selected);border-color:var(--line-sel)' : 'background:transparent'};height:28px">${c}</span>`).join('');
  const header = `<div class="sec-h" style="margin-bottom:16px;flex-wrap:wrap"><div style="display:flex;gap:4px">${chips}</div><span class="sp"></span><span class="caption t3">5 installed · 2 updates</span><span class="icon-btn">${iconSvg('refresh-cw', 16, T.text2)}</span>${width >= 800 ? `<div class="search">${iconSvg('search', 14, T.text3)}<span>Search apps</span></div>` : ''}</div>`;
  const tilesOf = (list) => grid(cols, list.map((a) => cardTile(a, T, { artH })));
  const content = `${header}${secHead('Installed', 5)}${tilesOf(installed)}<div style="height:24px"></div>${secHead('Available', 3)}${tilesOf(available)}${showPlanned ? `<div style="height:24px"></div>${secHead('Planned · study only, not part of the catalog yet', planned.length)}${tilesOf(planned)}` : ''}`;
  return page(T, controlWindow(T, content, { width, side, pad, title }));
}

export function layoutBDetail(T) {
  const a = byId.calendar;
  const st = stateOf(a, 'update');
  const panel = `<aside style="width:400px;flex:none;border-left:1px solid var(--line);background:var(--panel);display:flex;flex-direction:column">
    <div class="top" style="justify-content:space-between"><span class="strong">${a.name}</span><span class="icon-btn" style="width:28px;height:28px">${iconSvg('x', 16, T.text2)}</span></div>
    <div style="aspect-ratio:2/1;border-bottom:1px solid var(--line)">${artSvg('calendar', T, { w: '100%', h: '100%' })}</div>
    <div style="padding:16px;display:flex;flex-direction:column;gap:12px">
      <div class="id" style="gap:12px">${tile(a, T, 36)}<div><div class="nm">${a.name}</div><div class="meta t3">${a.q}</div></div>${pillHtml(st, T)}</div>
      <p class="t2">${a.s}</p>
      <div style="display:flex;gap:8px"><span class="btn">Update to ${a.ver}</span><span class="btn ghost">Release notes</span></div>
      <div style="border-top:1px solid var(--line);margin-top:4px"></div>
      ${[['Version', `${a.from} › ${a.ver}`], ['Download', a.size], ['Window class', a.cls], ['Needs', 'Core service (included)'], ['Rights', 'Calendar · read, write']].map(([k, v]) => `<div style="display:flex;gap:12px"><span class="meta t3" style="width:96px">${k}</span><span class="meta tab">${esc(v)}</span></div>`).join('')}
    </div></aside>`;
  const chips = ['All', 'Plan', 'Inbox', 'People', 'AI'].map((c, i) => `<span class="btn" style="${i === 0 ? 'background:var(--selected);border-color:var(--line-sel)' : 'background:transparent'};height:28px">${c}</span>`).join('');
  const sel = (a2) => cardTile(a2, T, { artH: 120, cls: a2.id === 'calendar' ? 'hover' : '' });
  const inner = `<div style="display:flex;gap:4px;margin-bottom:16px">${chips}</div>${secHead('Installed', 5)}${grid(2, installed.map(sel))}<div style="height:24px"></div>${secHead('Available', 3)}${grid(2, available.slice(0, 2).map(sel))}`;
  const win = `<div class="win" style="width:1600px;height:1000px;grid-template-columns:240px 1fr"><aside class="side"><div class="top"><span class="strong" style="margin-left:56px">Control centre</span>${iconSvg('panel-left', 16, T.text2)}</div><nav class="nav">${['layout-grid', 'cloud', 'sparkles', 'code', 'link', 'monitor', 'user-check', 'layers', 'sun', 'eye-off'].map((ic, i) => `<a class="${i ? '' : 'on'}">${iconSvg(ic, 16, i ? T.text2 : T.text)}<span>${['Overview', 'Accounts', 'AI', 'API clients', 'MCP clients', 'MCP servers', 'Approvals', 'Workspaces', 'Appearance', 'Privacy'][i]}</span></a>`).join('')}</nav></aside>
    <div class="content" style="overflow:hidden"><div class="top"><span class="strong">Overview</span><span style="flex:1"></span><span class="btn primary sm">Update all (2)</span></div><div style="display:flex;min-height:0;flex:1"><div class="page" style="flex:1;overflow:hidden">${inner}</div>${panel}</div></div></div>`;
  return page(T, win);
}

export function layoutC(T) {
  const a = byId.calendar;
  const st = stateOf(a, 'update');
  const group = (title, list, sel) => `${secHead(title, list.length)}<div style="display:flex;flex-direction:column;gap:8px;margin-bottom:24px">${list.map((x) => cardRow(x, T, { selected: x.id === sel })).join('')}</div>`;
  const left = `<div style="width:440px;flex:none;padding:24px 16px 24px 24px;border-right:1px solid var(--line);overflow:hidden">${appsHeader(T, 5, 2, true).replace('width:240px', 'width:200px')}${group('Installed', installed, 'calendar')}${group('Available', available)}${group('Planned', planned.slice(0, 3))}</div>`;
  const win = `<div style="border:1px solid var(--line-strong);border-radius:4px;overflow:hidden;background:var(--panel);box-shadow:0 12px 32px var(--shadow)">
      <div class="top" style="background:var(--raised)"><span class="lights"><i></i><i></i><i></i></span><span class="caption t3">Torro Calendar</span></div>
      <div style="aspect-ratio:16/9;background:var(--bg)">${artSvg('calendar', T, { w: '100%', h: '100%', slice: true, view: [0, 0, 400, 200] })}</div></div>`;
  const right = `<div style="flex:1;padding:32px;display:flex;flex-direction:column;gap:20px;min-width:0">
    ${win}
    <div class="id" style="gap:16px">${tile(a, T, 48)}<div><div class="title">${a.name}</div><div class="t3">${a.q}</div></div>${pillHtml(st, T)}</div>
    <p class="t2" style="max-width:62ch">${a.s}</p>
    <div style="display:flex;gap:8px"><span class="btn">Update to ${a.ver}</span><span class="btn ghost">Release notes</span><span class="btn ghost">Show in Finder</span></div>
    <div style="display:grid;grid-template-columns:repeat(4,1fr);gap:12px">${[['Version', `${a.from} › ${a.ver}`], ['Download', a.size], ['Window class', a.cls], ['Needs', 'Core service']].map(([k, v]) => `<div class="card" style="padding:12px 16px;background:var(--raised)"><div class="caption t3">${k}</div><div class="strong tab ell">${esc(v)}</div></div>`).join('')}</div></div>`;
  const win2 = `<div class="win" style="width:1600px;height:1000px;grid-template-columns:240px 1fr"><aside class="side"><div class="top"><span class="strong" style="margin-left:56px">Control centre</span>${iconSvg('panel-left', 16, T.text2)}</div><nav class="nav">${['Overview', 'Accounts', 'AI', 'API clients', 'MCP clients', 'MCP servers', 'Approvals', 'Workspaces', 'Appearance', 'Privacy'].map((n, i) => `<a class="${i ? '' : 'on'}">${iconSvg(['layout-grid', 'cloud', 'sparkles', 'code', 'link', 'monitor', 'user-check', 'layers', 'sun', 'eye-off'][i], 16, i ? T.text2 : T.text)}<span>${n}</span></a>`).join('')}</nav></aside><div class="content" style="overflow:hidden"><div class="top"><span class="strong">Overview</span><span style="flex:1"></span><span class="btn primary sm">Update all (2)</span></div><div style="display:flex;flex:1;min-height:0">${left}${right}</div></div></div>`;
  return page(T, win2);
}

// ---------- 3. Systemtafeln ----------
export function anatomy(T) {
  const a = byId.tasks;
  const n = (k, style) => `<span style="position:absolute;${style};width:20px;height:20px;border-radius:10px;background:var(--text);color:var(--bg);font:600 11px/20px Inter;text-align:center;z-index:5">${k}</span>`;
  const card = `<div style="position:relative;width:360px">${cardTile(a, T, { width: '360px', artH: 160, state: 'update' })}
    ${n(1, 'left:-28px;top:60px')}${n(2, 'left:-28px;top:150px')}${n(3, 'right:-28px;top:196px')}${n(4, 'right:-28px;top:222px')}${n(5, 'left:-28px;top:262px')}${n(6, 'left:-28px;top:330px')}${n(7, 'right:-28px;top:330px')}</div>`;
  const spec = [
    ['1', 'Image area', 'Art 400 × 200 drawing, cropped to 150 high in the grid (2:1 in the media card). Tinted ground, glow, dot grid, abstract UI scene. Never a photo, never text of its own.'],
    ['2', 'App tile', '44 (grid) / 48 (detail) / 36 (row). Tinted app colour (18 %), icon in the full colour, radius 4 (`TileSize` family, larger step). Overlaps the image by 22.'],
    ['3', 'Name', '`typo::body_strong()` 14/22, single line with “…”.'],
    ['4', 'Question', '`typo::meta()` 13/18 in `TEXT_3`, single line. It is the app’s one question, as in the catalog today.'],
    ['5', 'Description', '`typo::meta()` 13/18 in `TEXT_2`, at most two lines.'],
    ['6', 'State', 'Status pill: colour only in the dot, always with a word (Available, Installed, Update available, Managed by …).'],
    ['7', 'Action', 'One button `control_sm`: Install · size / Open / Update to x. Other actions in the “…” menu (card hover).'],
  ];
  const rows = spec.map(([k, t, d]) => `<div style="display:flex;gap:12px;margin-bottom:12px"><span style="flex:none;width:20px;height:20px;border-radius:10px;background:var(--text);color:var(--bg);font:600 11px/20px Inter;text-align:center">${k}</span><div><div class="strong">${t}</div><div class="meta t2" style="max-width:56ch">${d}</div></div></div>`).join('');
  const tokens = [['Card', 'RAISED, border LINE_STRONG, radius 4, hover border LINE_SELECTED, focus FOCUS_RING 2 px + 1 px gap'], ['Padding', '16 (panel_padding) · body gap 8 · grid gap 16 (card_gap 12 in dense)'], ['Art height', '150 grid · 200 media · 120 detail list'], ['Motion', 'none; no parallax, no looping animation (calm, always-on)']];
  const body = `<div style="padding:40px 56px;display:flex;gap:64px;width:1100px"><div>${card}</div><div style="flex:1"><div class="title" style="margin-bottom:16px">Card anatomy</div>${rows}<div style="border-top:1px solid var(--line);margin:16px 0"></div>${tokens.map(([k, v]) => `<div style="display:flex;gap:12px"><span class="meta t3" style="width:84px;flex:none">${k}</span><span class="meta">${v}</span></div>`).join('')}</div></div>`;
  return page(T, body);
}

export function states(T) {
  const t = byId.tasks;
  const items = [['Available', 'available', ''], ['Hover', 'available', 'hover'], ['Focus', 'available', 'focus'], ['Installing', 'running', ''], ['Update available', 'update', ''], ['Installed', 'installed', ''], ['Managed by a package manager', 'managed', ''], ['Turned off', 'off', ''], ['Not available here', 'unavailable', ''], ['Install failed', 'error', '']];
  const cell = ([label, st, cls]) => `<div><div class="caption t3" style="margin-bottom:8px">${label}</div>${cardTile(t, T, { state: st, cls, artH: 110 })}</div>`;
  const nb = byId.notes;
  const extra = `<div><div class="caption t3" style="margin-bottom:8px">Planned (study only)</div>${cardTile(nb, T, { artH: 110 })}</div>`;
  const body = `<div style="padding:32px;width:1500px"><div class="title" style="margin-bottom:4px">Card states</div><div class="meta t2" style="margin-bottom:24px">One app (Tasks) in every state, so only the state changes. Colour carries no meaning on its own: each state has a word or icon.</div>${grid(4, [...items.map(cell), extra])}</div>`;
  return page(T, body);
}

export function hues() {
  const rows = (T) => APPS.map((a) => {
    const h = hue(a, T), tinted = mix(h, T.panel, T.tint + 0.02), ground = mix(h, T.panel, T.name === 'dark' ? 0.07 : 0.05);
    const cIcon = contrast(h, tinted), cPanel = contrast(h, T.panel), cText = contrast(T.text, ground);
    return { id: a.id, name: a.name, h, cIcon, cPanel, cText };
  });
  return { dark: rows(THEMES.dark), light: rows(THEMES.light) };
}
export function hueSheet(T) {
  const data = hues()[T.name];
  const tr = data.map((r) => {
    const a = byId[r.id];
    const ok = (v) => `<span class="tab" style="color:${v >= 3 ? T.ok : T.error}">${v.toFixed(1)}:1</span>`;
    return `<tr><td>${tile(a, T, 28)}</td><td class="strong">${a.short}</td><td><span style="display:inline-block;width:56px;height:16px;border-radius:4px;background:${r.h}"></span> <span class="meta t3 tab">${r.h}</span></td><td>${ok(r.cIcon)}</td><td>${ok(r.cPanel)}</td><td class="tab t2">${r.cText.toFixed(1)}:1</td><td class="meta t3">${a.status === 'catalog' || a.status === 'host' ? 'exists' : 'planned'}</td></tr>`;
  }).join('');
  const body = `<div style="padding:32px;width:980px"><div class="title" style="margin-bottom:4px">App colours · ${T.name}</div><div class="meta t2" style="margin-bottom:16px">Icon colour on its own tint (the tile) and on the panel, as a mark (≥ 3:1). Text on the art ground is the normal text colour.</div>
  <table style="border-collapse:collapse;width:100%"><thead><tr class="caption t3" style="text-align:left"><th></th><th>App</th><th style="text-align:left">Colour</th><th>Icon on tint</th><th>Icon on panel</th><th>Text on ground</th><th>Status</th></tr></thead><tbody>${tr.replace(/<td/g, '<td style="padding:6px 12px 6px 0;border-top:1px solid var(--line)"')}</tbody></table></div>`;
  return page(T, body);
}

export function artLayers(appId, T) {
  const a = byId[appId];
  const h = hue(a, T);
  const full = artSvg(appId, T, { w: 300, h: 150 });
  const only = (what) => {
    const base = mix(h, T.panel, T.name === 'dark' ? 0.07 : 0.05);
    const g = `<defs><radialGradient id="x1" cx="78%" cy="8%" r="85%"><stop offset="0" stop-color="${h}" stop-opacity="${T.name === 'dark' ? 0.34 : 0.26}"/><stop offset="1" stop-color="${h}" stop-opacity="0"/></radialGradient><pattern id="x2" width="14" height="14" patternUnits="userSpaceOnUse"><circle cx="1" cy="1" r=".8" fill="${T.text3}" opacity=".3"/></pattern></defs>`;
    const layers = { 1: `<rect width="400" height="200" fill="${base}"/>`, 2: `<rect width="400" height="200" fill="${base}"/><rect width="400" height="200" fill="url(#x1)"/>`, 3: `<rect width="400" height="200" fill="${base}"/><rect width="400" height="200" fill="url(#x1)"/><rect width="400" height="200" fill="url(#x2)" opacity=".55"/>` };
    return `<svg width="300" height="150" viewBox="0 0 400 200">${g}${layers[what]}</svg>`;
  };
  const cell = (n, t, d, svg) => `<div style="width:300px"><div style="border:1px solid var(--line-strong);border-radius:4px;overflow:hidden">${svg}</div><div class="strong" style="margin-top:8px">${n} ${t}</div><div class="meta t2">${d}</div></div>`;
  const body = `<div style="padding:32px;width:1740px"><div class="title" style="margin-bottom:4px">How a motif is built · ${a.name}</div><div class="meta t2" style="margin-bottom:24px">Four layers, the same for every app. Only the scene (4) and the colour change; that is what keeps the gallery one family.</div>
  <div style="display:flex;gap:24px;align-items:flex-start">${cell(1, 'Ground', 'App colour at 5–7 % over the panel. Never a full colour surface.', only(1))}${cell(2, 'Glow', 'Radial glow in the app colour, top right, 26–34 %.', only(2))}${cell(3, 'Dot grid', 'Dots in TEXT_3 at 30 %, 14 px grid. Ties the family together.', only(3))}${cell(4, 'UI scene', 'Abstracted window parts in the real surface tokens, text only as lines, colour only in marks. The part that differs per app.', full)}${cell(5, 'App tile', 'Tinted tile with the Lucide icon, overlapping the edge (card) or leading the title (hero).', `<div style="position:relative;width:300px;height:150px">${full}<span style="position:absolute;left:16px;bottom:16px">${tile(a, T, 44)}</span></div>`)}</div></div>`;
  return page(T, body);
}

// ---------- 4. Welcome-Screens ----------
const GENERIC_NAV = [['Today', 0], ['Recent', 0], ['Starred', 0]];
export function welcome(app, T, { width = 1440, height = 820 } = {}) {
  const w = app.welcome;
  const h = hue(app, T);
  const planned = app.status === 'planned' || app.status === 'parked';
  const side = `<aside class="side"><div class="top"><span class="strong" style="margin-left:56px">${esc(app.short)}</span>${iconSvg('panel-left', 16, T.text2)}</div>
    <div style="padding:8px 0;display:flex;flex-direction:column;gap:2px">${GENERIC_NAV.map(([n], i) => `<div style="height:32px;display:flex;align-items:center;gap:8px;padding:0 8px;border-radius:4px;${i === 0 ? 'background:var(--selected)' : ''}">${iconSvg(i === 0 ? app.icon : ['clock', 'star'][i - 1], 16, i === 0 ? T.text : T.text3)}<span class="${i === 0 ? '' : 't3'}" style="${i === 0 ? '' : 'opacity:.7'}">${['Welcome', 'Recent', 'Favourites'][i]}</span></div>`).join('')}</div>
    <div style="flex:1"></div><div class="meta t3" style="padding:8px">No account connected</div></aside>`;
  const hero = `<section style="position:relative;height:300px;border-bottom:1px solid var(--line);overflow:hidden;background:var(--panel)">
    <div style="position:absolute;right:0;top:0;bottom:0;width:62%">${artSvg(app.id, T, { w: '100%', h: '100%', slice: true })}</div>
    <div style="position:absolute;inset:0;background:linear-gradient(90deg,var(--panel) 0%,var(--panel) 36%,transparent 70%)"></div>
    <div style="position:relative;padding:40px 48px;width:56%;display:flex;flex-direction:column;gap:12px;height:100%;justify-content:center">
      <div style="display:flex;align-items:center;gap:12px">${tile(app, T, 48)}<span class="caption t3">Welcome to ${esc(app.name)}</span></div>
      <h1 class="headline">${esc(w.headline)}</h1>
      <p class="t2" style="max-width:46ch">${esc(w.sub)}</p>
      <div style="display:flex;gap:8px;margin-top:8px"><span class="btn primary" style="height:40px">${esc(w.actions[0].t)}</span><span class="btn ghost" style="height:40px">Skip for now</span></div>
    </div></section>`;
  const actions = w.actions.map((x) => `<div class="card" style="flex-direction:row;align-items:center;gap:16px;padding:16px;border-radius:4px">
    <span class="tile" style="width:36px;height:36px;background:${rgba(h, T.tint + 0.02)}">${iconSvg(x.icon, 18, h)}</span>
    <div style="flex:1;min-width:0"><div class="strong">${esc(x.t)}</div><div class="meta t3">${esc(x.d)}</div></div>${x.k ? `<span class="kbd">${esc(x.k)}</span>` : iconSvg('chevron-right', 16, T.text3)}</div>`).join('');
  const peek = `<div class="card" style="background:var(--panel)"><div style="padding:12px 16px;display:flex;align-items:center;gap:8px;border-bottom:1px solid var(--line)"><span class="caption t3">${esc(w.peek.title)}</span><span class="sp" style="flex:1"></span><span class="pill dash" style="height:20px">Example</span></div>
    ${w.peek.rows.map(([a, b, c], i) => `<div style="height:48px;padding:0 16px;display:flex;align-items:center;gap:12px;${i ? 'border-top:1px solid var(--line)' : ''};opacity:.62"><span style="width:8px;height:8px;border-radius:50%;background:${i === 0 ? h : T.text3};opacity:${i === 0 ? 1 : .5}"></span><div style="flex:1;min-width:0"><div class="strong ell">${esc(a)}</div><div class="meta t3 ell">${esc(b)}</div></div><span class="meta t3 tab">${esc(c)}</span></div>`).join('')}</div>`;
  const tips = w.tips.map((t) => `<div style="display:flex;gap:8px;align-items:center" class="meta t2">${iconSvg('lightbulb', 14, h)}${esc(t)}</div>`).join('');
  const page2 = `${hero}<div style="padding:32px 48px;display:grid;grid-template-columns:1.15fr 1fr;gap:32px;flex:1">
    <div><div class="caption t3" style="margin-bottom:12px">Get started</div><div style="display:flex;flex-direction:column;gap:12px">${actions}</div></div>
    <div><div class="caption t3" style="margin-bottom:12px">What will appear here</div>${peek}<div style="display:flex;flex-direction:column;gap:8px;margin-top:16px">${tips}</div></div></div>
    <div class="meta t3" style="padding:0 48px 20px;display:flex;gap:8px;align-items:center">${iconSvg('shield', 14, T.text3)}Nothing leaves your device until you connect an account.${planned ? ' · Concept: this app does not exist yet.' : ''}</div>`;
  const win = `<div class="win" style="width:${width}px;height:${height}px;grid-template-columns:240px 1fr">${side}<div class="content" style="min-height:0"><div class="top"><span class="strong">${esc(app.short)}</span><span style="flex:1"></span><span class="icon-btn" style="width:28px;height:28px">${iconSvg('sliders-horizontal', 16, T.text2)}</span></div>${page2}</div></div>`;
  return page(T, win);
}

/** Hero-Varianten: wiederverwendbare Kopfbänder derselben Bildsprache (L: Start, M: Band, S: nur Titel). */
export function heroVariants(app, T) {
  const h = hue(app, T);
  const band = (hh, content) => `<div style="position:relative;height:${hh}px;border:1px solid var(--line-strong);border-radius:4px;overflow:hidden;background:var(--panel)"><div style="position:absolute;right:0;top:0;bottom:0;width:60%">${artSvg(app.id, T, { w: '100%', h: '100%', slice: true })}</div><div style="position:absolute;inset:0;background:linear-gradient(90deg,var(--panel) 0%,var(--panel) 38%,transparent 72%)"></div><div style="position:relative;padding:0 32px;height:100%;display:flex;align-items:center;gap:16px">${content}</div></div>`;
  const body = `<div style="padding:32px;width:1200px;display:flex;flex-direction:column;gap:32px">
   <div><div class="caption t3" style="margin-bottom:8px">Hero L · first start / empty state · 300 high · one primary button</div>${band(300, `<div style="display:flex;flex-direction:column;gap:12px">${tile(app, T, 48)}<div class="headline">${esc(app.welcome.headline)}</div><div class="t2" style="max-width:44ch">${esc(app.welcome.sub)}</div><div><span class="btn primary" style="height:40px">${esc(app.welcome.actions[0].t)}</span></div></div>`)}</div>
   <div><div class="caption t3" style="margin-bottom:8px">Hero M · “what’s new” or a new feature · 120 high · dismissible</div>${band(120, `${tile(app, T, 44)}<div><div class="strong">What’s new in ${esc(app.short)}</div><div class="meta t2" style="margin-bottom:4px">Short sentence about the release. One text link at most.</div><span class="btn sm" style="height:28px">Release notes</span></div>`)}</div>
   <div><div class="caption t3" style="margin-bottom:8px">Hero S · every normal start · no art, only the tile in the header</div><div class="top" style="border:1px solid var(--line-strong);border-radius:4px;background:var(--panel);height:40px">${tile(app, T, 24)}<span class="strong">${esc(app.short)}</span><span class="t3 meta">${esc(app.q)}</span></div></div></div>`;
  return page(T, body);
}
