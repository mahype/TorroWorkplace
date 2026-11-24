// UI-Bausteine der Mockups als HTML: gemeinsames Kartensystem (Karte „Medien“, „Kachel“, „Zeile“), Zustände,
// Fensterrahmen der Steuerzentrale. Maße und Farben folgen dem Skill torro-design (Standardpaket `torro`):
// Radius 4, Haarlinien, Text 14/22, Abstände 4/8/12/16/24/32. Neu im Vorschlag sind nur: die Bildfläche der Karte,
// die Appfarbe (getönt, nie als volle Fläche) und die größere Kachel (48) – beides Entscheidungspunkte im README.
import { THEMES, rgba, iconSvg, esc } from './kit.mjs';
import { APPS, byId } from './apps.mjs';
import { artSvg } from './art.mjs';

const FONT = '../../../../crates/torro-gui/assets/fonts';

export function vars(T) {
  return `--bg:${T.bg};--panel:${T.panel};--raised:${T.raised};--hover:${T.hover};--selected:${T.selected};--menu:${T.menu};--text:${T.text};--text2:${T.text2};--text3:${T.text3};--line:${T.line};--line-strong:${T.lineStrong};--line-sel:${T.lineSel};--accent:${T.accent};--ok:${T.ok};--attention:${T.attention};--error:${T.error};--shadow:${T.shadow};color-scheme:${T.name};`;
}

export const css = (T) => `
@font-face{font-family:Inter;font-weight:400;src:url(${FONT}/Inter-Regular.ttf)}
@font-face{font-family:Inter;font-weight:500;src:url(${FONT}/Inter-Medium.ttf)}
@font-face{font-family:Inter;font-weight:600;src:url(${FONT}/Inter-SemiBold.ttf)}
:root{${vars(T)}}
*{box-sizing:border-box;margin:0;padding:0}
body{background:var(--bg);color:var(--text);font:400 14px/22px Inter,system-ui,sans-serif;-webkit-font-smoothing:antialiased}
.micro{font-size:11px;line-height:14px}.caption{font-size:12px;line-height:16px;font-weight:500}.meta{font-size:13px;line-height:18px}
.strong{font-weight:500}.title{font-size:22px;line-height:28px;font-weight:600}.headline{font-size:28px;line-height:36px;font-weight:600;letter-spacing:-.01em}
.t2{color:var(--text2)}.t3{color:var(--text3)}.tab{font-variant-numeric:tabular-nums}
.ell{white-space:nowrap;overflow:hidden;text-overflow:ellipsis;min-width:0}
svg{display:block;flex:none}
/* Fensterrahmen */
.win{background:var(--panel);display:grid;grid-template-columns:240px 1fr;border:1px solid var(--line-strong);border-radius:4px;overflow:hidden}
.side{background:var(--bg);border-right:1px solid var(--line);display:flex;flex-direction:column;padding:0 8px 12px;min-height:100%}
.top{height:40px;display:flex;align-items:center;gap:8px;padding:0 8px;border-bottom:1px solid var(--line)}
.side .top{border:0;padding:0 8px;justify-content:space-between}
.lights{display:flex;gap:8px;margin-right:4px}.lights i{width:12px;height:12px;border-radius:50%;background:var(--text3);opacity:.35}
.nav{display:flex;flex-direction:column;gap:2px;margin-top:8px}
.nav a{height:32px;display:flex;align-items:center;gap:8px;padding:0 8px;border-radius:4px;color:var(--text2);text-decoration:none}
.nav a.on{background:var(--selected);color:var(--text)}.nav a .n{margin-left:auto;font-size:13px;color:var(--text3)}
.content{min-width:0;display:flex;flex-direction:column}
.page{padding:24px}
.sec-h{display:flex;align-items:center;gap:12px;margin:0 0 12px}.sec-h .sp{flex:1}
.search{width:240px;height:36px;border:1px solid var(--line-strong);border-radius:4px;padding:0 12px;display:flex;align-items:center;gap:8px;color:var(--text3);background:var(--panel)}
.icon-btn{width:36px;height:36px;border-radius:4px;display:grid;place-items:center}
/* Knöpfe, Pillen */
.btn{height:36px;padding:0 12px;border-radius:4px;border:1px solid var(--line-strong);background:var(--raised);color:var(--text);font:500 12px/16px Inter;display:inline-flex;align-items:center;gap:8px;white-space:nowrap}
.btn.primary{background:var(--accent);border-color:transparent;color:#fff}.btn.ghost{background:transparent;border-color:transparent;color:var(--text2)}
.btn[disabled],.btn.off{opacity:.4}.btn.sm{height:28px}
.pill{height:22px;border-radius:11px;border:1px solid var(--line-strong);background:var(--raised);padding:0 8px;display:inline-flex;align-items:center;gap:6px;font:500 12px/16px Inter;color:var(--text);white-space:nowrap}
.pill i{width:6px;height:6px;border-radius:50%;background:var(--text3)}.pill.dash{border-style:dashed;background:transparent;color:var(--text2)}
.kbd{height:18px;padding:0 5px;border:1px solid var(--line-strong);border-radius:4px;font:500 11px/16px Inter;color:var(--text3);display:inline-flex;align-items:center}
.tile{display:grid;place-items:center;border-radius:4px;flex:none}
.bar{height:4px;border-radius:2px;background:var(--line-strong);overflow:hidden}.bar i{display:block;height:100%;background:var(--ok)}
/* Karten: gemeinsames System */
.card{background:var(--raised);border:1px solid var(--line-strong);border-radius:4px;overflow:hidden;display:flex;flex-direction:column;position:relative;min-width:0}
.card.hover{border-color:var(--line-sel);box-shadow:0 6px 18px var(--shadow)}.card.focus{outline:2px solid ${T.name === 'dark' ? 'rgba(255,255,255,.35)' : 'rgba(0,0,0,.5)'};outline-offset:1px}
.card.planned{border-style:dashed;background:transparent}
.card .art{border-bottom:1px solid var(--line);display:block;overflow:hidden;background:var(--panel)}
.card.dim .art{filter:saturate(.35);opacity:.8}
.id{display:flex;align-items:center;gap:12px;min-width:0}.id .nm{font-weight:500;color:var(--text)}.id>div{min-width:0;flex:1}
.card .s{color:var(--text2);font-size:13px;line-height:18px;display:-webkit-box;-webkit-line-clamp:2;-webkit-box-orient:vertical;overflow:hidden}
.foot{display:flex;align-items:center;gap:12px;justify-content:space-between}
.card .cmd{height:36px;border:1px solid var(--line-strong);border-radius:4px;padding:0 12px;display:flex;align-items:center;font:400 13px/18px ui-monospace,Menlo,monospace;color:var(--text2);flex:1;background:var(--panel)}
/* Medien-Karte (Variante A, auch Basis der Feature-Karte) */
.media .body{padding:16px;display:flex;flex-direction:column;gap:12px}
/* Kachel-Karte (Variante B): Kachel überlappt die Bildfläche */
.tilecard .art{position:relative;overflow:visible}.tilecard .artin{overflow:hidden;height:100%}.tilecard .lead{position:absolute;left:16px;bottom:-22px;z-index:2;border-radius:6px;border:3px solid var(--raised);background:var(--raised)}
.tilecard .body{padding:30px 16px 16px;display:flex;flex-direction:column;gap:8px;flex:1}
.tilecard .foot{margin-top:auto;padding-top:8px}
.rowcard{flex-direction:row;align-items:center;gap:12px;padding:12px 16px}
.chip-fam{position:absolute;top:8px;left:8px;z-index:3;height:20px;padding:0 8px;border-radius:10px;background:rgba(0,0,0,.45);color:#fff;font:500 11px/20px Inter;backdrop-filter:blur(6px)}
`;

/** Eine Appkachel (tinted tile): Icon in voller Appfarbe auf getöntem Grund, Radius 4. */
export function tile(app, T, s = 36) {
  const h = app.hue[T.name];
  const icon = Math.round(s * 0.5);
  const dashed = app.status === 'planned' || app.status === 'parked';
  return `<span class="tile" style="width:${s}px;height:${s}px;background:${rgba(h, T.tint + 0.02)};${dashed ? `outline:1px dashed ${rgba(h, 0.5)};outline-offset:-1px;` : ''}">${iconSvg(app.icon, icon, h)}</span>`;
}

const STATES = {
  available: { dot: 'var(--text3)', label: 'Available', btn: (a) => `Install${a.size ? ` · ${a.size}` : ''}`, ver: (a) => `${a.ver} · ${a.cls}` },
  installed: { dot: 'var(--ok)', label: 'Installed', btn: () => 'Open', ver: (a) => `${a.ver} · ${a.cls}` },
  update: { dot: 'var(--attention)', label: 'Update available', btn: (a) => `Update to ${a.ver}`, ver: (a) => `${a.from} › ${a.ver} · ${a.cls}` },
  managed: { dot: 'var(--text3)', label: 'Managed by AUR', btn: () => 'Copy command', ver: (a) => `${a.from} › ${a.ver} · ${a.cls}`, icon: 'package' },
  off: { dot: 'var(--text3)', label: 'Turned off', btn: () => 'Turn on', ver: (a) => `${a.ver} · ${a.cls}` },
  unavailable: { dot: 'var(--text3)', label: 'Not available for this system', btn: null, ver: (a) => `${a.ver} · ${a.cls}` },
  running: { dot: 'var(--attention)', label: 'Installing…', btn: () => 'Installing…', ver: (a) => `${a.ver} · ${a.cls}`, icon: 'refresh-cw', busy: true },
  error: { dot: 'var(--error)', label: 'Install failed', btn: () => 'Retry', ver: (a) => `${a.ver} · ${a.cls}` },
  host: { dot: 'var(--ok)', label: 'Installed', btn: () => 'Open', ver: (a) => `Version ${a.ver} · ${a.cls}` },
  planned: { dot: 'var(--text3)', label: 'Planned', dash: true, btn: null, ver: (a) => (a.src ? a.src.split(',')[0] : 'Concept') },
};
export const stateOf = (app, force) => STATES[force ?? app.cardState] ?? STATES.available;

export function pillHtml(st, T) {
  const ic = st.icon ? iconSvg(st.icon, 12, 'currentColor') : `<i style="background:${st.dot}"></i>`;
  return `<span class="pill${st.dash ? ' dash' : ''}">${ic}${esc(st.label)}</span>`;
}
const btnHtml = (st, app, kind = '') => (st.btn ? `<span class="btn${st.busy ? ' off' : ''} ${kind}">${st.busy ? iconSvg('refresh-cw', 14, 'currentColor') : ''}${esc(st.btn(app))}</span>` : '');
const verHtml = (st, app) => `<span class="meta t3 tab ell">${esc(st.ver(app))}</span>`;
const nameHtml = (app) => `<div class="nm ell">${esc(app.name)}</div><div class="meta t3 ell">${esc(app.q)}</div>`;

/** Variante A: Medien-Karte, Bildfläche 2:1 oben, Text darunter. */
export function cardMedia(app, T, { state, cls = '', width = '100%', artH } = {}) {
  const st = stateOf(app, state);
  const planned = app.status === 'planned' || app.status === 'parked';
  const art = artSvg(app.id, T, { w: '100%', h: artH ?? '100%', slice: !!artH });
  return `<article class="card media ${planned ? 'dim' : ''} ${cls}" style="width:${width}">
  <div class="art" style="${artH ? `height:${artH}px` : 'aspect-ratio:2/1'}">${art}</div>
  <div class="body">
    <div class="id">${tile(app, T, 36)}<div>${nameHtml(app)}</div>${pillHtml(st, T)}</div>
    <p class="s">${esc(app.s)}</p>
    ${state === 'managed' || (!state && app.cardState === 'managed') ? `<div class="foot"><div class="cmd">yay -Syu</div><span class="icon-btn" style="border:1px solid var(--line-strong)">${iconSvg('copy', 16, T.text2)}</span></div>` : ''}
    <div class="foot">${verHtml(st, app)}${btnHtml(st, app)}</div>
  </div></article>`;
}

/** Variante B: kompakte Kachel-Karte, Bildfläche 150 hoch, überlappende Appkachel (Creative-Suite-Muster). */
export function cardTile(app, T, { state, cls = '', width = '100%', artH = 150, fam = true } = {}) {
  const st = stateOf(app, state);
  const planned = app.status === 'planned' || app.status === 'parked';
  const art = artSvg(app.id, T, { w: '100%', h: artH, slice: true });
  return `<article class="card tilecard ${planned ? 'dim planned' : ''} ${cls}" style="width:${width}">
  <div class="art" style="height:${artH}px"><div class="artin">${art}</div><span class="lead">${tile(app, T, 44)}</span></div>
  <div class="body">
    <div class="id"><div>${nameHtml(app)}</div></div>
    <p class="s">${esc(app.s)}</p>
    ${state === 'running' ? `<div><div class="bar"><i style="width:42%;background:var(--attention)"></i></div><div class="meta t3 tab" style="margin-top:4px">Downloading · 10.3 of 24.6 MB</div></div>` : ''}
    ${state === 'error' ? `<div class="meta" style="display:flex;gap:8px;align-items:center;color:var(--text2)">${iconSvg('circle-x', 14, T.error)}The download did not finish. Check your connection.</div>` : ''}
    <div class="foot">${pillHtml(st, T)}${btnHtml(st, app)}</div>
  </div></article>`;
}

/** Variante C (Liste): eine Zeile je App, ohne Bildfläche; die Vorschau steht rechts im Detail. */
export function cardRow(app, T, { state, selected = false } = {}) {
  const st = stateOf(app, state);
  return `<div class="card rowcard" style="${selected ? `background:var(--selected);border-color:var(--line-sel)` : ''}">
  ${selected ? '<i style="position:absolute;left:0;top:0;bottom:0;width:2px;background:var(--accent)"></i>' : ''}
  ${tile(app, T, 36)}<div class="id" style="flex:1"><div>${nameHtml(app)}</div></div>${pillHtml(st, T)}</div>`;
}

const NAV = [['layout-grid', 'Overview', '', 1], ['cloud', 'Accounts', 7], ['sparkles', 'AI', 2], ['code', 'API clients', 1], ['link', 'MCP clients', 2], ['monitor', 'MCP servers', 5], ['user-check', 'Approvals', 3], ['layers', 'Workspaces', 3], ['sun', 'Appearance', ''], ['eye-off', 'Privacy', '']];

/** Fenster der Steuerzentrale um einen Inhalt. side: 'full' (240), 'rail' (60, nur Icons) oder 'none' (Schmal, Menü in der Zeile). */
export function controlWindow(T, content, { width = 1600, height, side = 'full', title = 'Overview', updateAll = true, pad = 24 } = {}) {
  const rail = side === 'rail';
  const nav = NAV.map(([ic, n, c, on]) => `<a class="${on ? 'on' : ''}" ${rail ? 'style="justify-content:center;padding:0"' : ''}>${iconSvg(ic, 16, on ? T.text : T.text2)}${rail ? '' : `<span>${n}</span>`}${!rail && c !== '' && !on ? `<span class="n tab">${c}</span>` : ''}</a>`).join('');
  const left = side === 'none' ? `${iconSvg('panel-left', 16, T.text2)}` : '';
  const menu = side === 'none' ? `<span class="strong">${title}</span>${iconSvg('chevron-down', 14, T.text3)}` : `${iconSvg('layout-grid', 16, T.text2)}<span class="strong">${title}</span>`;
  const btn = updateAll ? (width < 600 ? `<span class="btn primary sm" style="padding:0 8px">${iconSvg('download', 14, '#fff')}</span>` : '<span class="btn primary sm">Update all (2)</span>') : '';
  const top = `<div class="top">${left}${menu}<span style="flex:1"></span>${btn}</div>`;
  const cols = side === 'full' ? '240px 1fr' : side === 'rail' ? '60px 1fr' : '1fr';
  const aside = side === 'none' ? '' : `<aside class="side" ${rail ? 'style="padding:0 8px"' : ''}><div class="top">${rail ? `<span style="margin:0 auto">${iconSvg('panel-left', 16, T.text2)}</span>` : `<span class="strong" style="margin-left:56px">Control centre</span>${iconSvg('panel-left', 16, T.text2)}`}</div><nav class="nav">${nav}</nav><div style="flex:1"></div><nav class="nav"><a ${rail ? 'style="justify-content:center;padding:0"' : ''}>${iconSvg('sliders-horizontal', 16, T.text2)}${rail ? '' : '<span>General</span>'}</a></nav>${rail ? '' : '<div class="meta t3" style="padding:8px">Torro Workplace · Version 0.1.0</div>'}</aside>`;
  return `<div class="win" style="width:${width}px;${height ? `height:${height}px;` : ''}grid-template-columns:${cols}">${aside}<div class="content">${top}<div class="page" style="padding:${pad}px">${content}</div></div></div>`;
}

export function page(T, body, { w = 1600, bg = true, extraCss = '' } = {}) {
  return `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Torro App Gallery</title><style>${css(T)}${extraCss}</style></head><body style="${bg ? '' : 'background:transparent'}">${body}</body></html>`;
}
export { THEMES, APPS, byId };
