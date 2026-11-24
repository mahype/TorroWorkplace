// Erzeugt alle HTML-Seiten (html/) und rendert sie zu PNG. Aufruf (siehe README):
//   PLAYWRIGHT_DIR=<ordner mit playwright> node docs/prototypes/app-gallery/src/build.mjs [cards|layouts|system|welcome|html]
// Ohne Argument wird alles gebaut. Nur `html` schreibt die Seiten ohne zu rendern.
import { mkdirSync, writeFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { THEMES } from './kit.mjs';
import { APPS, real, byId } from './apps.mjs';
import { renderAll } from './render.mjs';
import { artSvg } from './art.mjs';
import { cardRow, controlWindow, page } from './ui.mjs';
import * as P from './pages.mjs';

const OUT = join(dirname(fileURLToPath(import.meta.url)), '..');
const want = new Set(process.argv.slice(2));
const on = (k) => want.size === 0 || want.has(k);
const jobs = [];
const themes = ['dark', 'light'];

function emit(group, name, html, { w, h, scale = 1 }) {
  const dir = join(OUT, 'html');
  mkdirSync(dir, { recursive: true });
  mkdirSync(join(OUT, group), { recursive: true });
  const f = join(dir, `${group}-${name}.html`);
  writeFileSync(f, html);
  jobs.push({ html: f, png: join(OUT, group, `${name}.png`), w, h, scale });
}

const miniList = (T) => {
  const body = `<div style="display:flex;flex-direction:column;gap:8px">${real.map((a) => cardRow(a, T)).join('')}</div>`;
  return page(T, controlWindow(T, `<div class="caption t3" style="margin-bottom:8px">Apps · 5 installed</div>${body}`, { width: 360, side: 'none', pad: 12, updateAll: false }));
};

for (const th of themes) {
  const T = THEMES[th];
  if (on('cards')) for (const a of APPS) emit('cards', `${a.id}-${th}`, P.cardSheet(a, T), { w: 944, scale: 1.5 });
  if (on('layouts')) {
    emit('layouts', `a-editorial-${th}`, P.layoutA(T), { w: 1600 });
    emit('layouts', `b-creative-suite-${th}`, P.layoutB(T), { w: 1600 });
    emit('layouts', `c-product-preview-${th}`, P.layoutC(T), { w: 1600, h: 1000 });
    emit('layouts', `b-detail-panel-${th}`, P.layoutBDetail(T), { w: 1600, h: 1000 });
    emit('layouts', `b-medium-${th}`, P.layoutB(T, { width: 800, cols: 2, side: 'rail', artH: 130, showPlanned: false }), { w: 800 });
    emit('layouts', `b-narrow-${th}`, P.layoutB(T, { width: 480, cols: 1, side: 'none', artH: 120, pad: 16, showPlanned: false, title: 'Overview' }), { w: 480 });
    emit('layouts', `b-narrow-360-${th}`, miniList(T), { w: 360 });
  }
  if (on('system')) {
    emit('system', `anatomy-${th}`, P.anatomy(T), { w: 1100, scale: 2 });
    emit('system', `states-${th}`, P.states(T), { w: 1500 });
    emit('system', `colours-${th}`, P.hueSheet(T), { w: 980, scale: 2 });
    emit('system', `motif-layers-calendar-${th}`, P.artLayers('calendar', T), { w: 1740 });
    emit('system', `hero-variants-calendar-${th}`, P.heroVariants(byId.calendar, T), { w: 1200 });
  }
  if (on('welcome')) for (const a of APPS) {
    if (th === 'light' && !(a.status === 'catalog' || a.status === 'host')) continue;
    emit('welcome', `${a.id}-${th}`, P.welcome(a, T), { w: 1440, h: 820 });
  }
}
if (on('art')) {
  // Die Motive als eigenständige SVGs (400 × 200): Vorlage für spätere Assets der App (siehe README, Umsetzungsskizze).
  mkdirSync(join(OUT, 'art'), { recursive: true });
  for (const th of themes) for (const a of APPS) writeFileSync(join(OUT, 'art', `${a.id}-${th}.svg`), artSvg(a.id, THEMES[th], { w: 400, h: 200 }));
}
if (want.has('html')) { console.log('html written'); process.exit(0); }
await renderAll(jobs);
