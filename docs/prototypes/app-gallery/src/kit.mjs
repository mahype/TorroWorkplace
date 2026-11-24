// Gemeinsame Bausteine der App-Galerie-Studie (Issue #970): Design-Tokens des Standardpakets (Skill
// torro-design, Abschnitt 1), Farbhelfer und kleine SVG-Zeichner. Nichts hier gehört zum Produkt; es
// erzeugt nur die Mockups. Die Werte sind die des Standards `torro` (designs/torro/design.toml).
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const here = dirname(fileURLToPath(import.meta.url));
export const ROOT = join(here, '..', '..', '..', '..');
const ICON_DIR = join(ROOT, 'crates', 'torro-gui', 'assets', 'icons');

/** Tokens je Erscheinungsbild (Skill torro-design, Tabelle „Farben“). */
export const THEMES = {
  dark: {
    name: 'dark', bg: '#08090A', panel: '#0F1011', raised: '#141518', hover: '#16171A', selected: '#1C1E22',
    menu: '#17181B', text: '#EDEEF0', text2: '#B4B8BF', text3: '#8A8F98',
    line: 'rgba(255,255,255,.06)', lineStrong: 'rgba(255,255,255,.10)', lineSel: 'rgba(255,255,255,.28)',
    accent: '#D50C0C', onAccent: '#FFFFFF', ok: '#3FB984', attention: '#E8A33D', error: '#F0645A',
    scrim: 'rgba(0,0,0,.62)', tint: 0.18, shadow: 'rgba(0,0,0,.55)',
    cal: ['#5B8CFF', '#3FB984', '#E8A33D', '#D96CA8', '#A78BFA'],
  },
  light: {
    name: 'light', bg: '#F4F5F6', panel: '#FFFFFF', raised: '#F8F9FA', hover: '#F0F1F3', selected: '#E9EAED',
    menu: '#FFFFFF', text: '#18191B', text2: '#4F535A', text3: '#62666D',
    line: 'rgba(0,0,0,.07)', lineStrong: 'rgba(0,0,0,.11)', lineSel: 'rgba(0,0,0,.32)',
    accent: '#D50C0C', onAccent: '#FFFFFF', ok: '#1F9D68', attention: '#B06F12', error: '#C7352B',
    scrim: 'rgba(0,0,0,.30)', tint: 0.14, shadow: 'rgba(16,24,40,.18)',
    cal: ['#3B6EF0', '#1F9D68', '#B06F12', '#C2508F', '#7C5CE6'],
  },
};

// ---------- Farbe ----------
const hex2rgb = (h) => {
  const s = h.replace('#', '');
  return [0, 2, 4].map((i) => parseInt(s.slice(i, i + 2), 16));
};
export const rgba = (hex, a) => {
  const [r, g, b] = hex2rgb(hex);
  return `rgba(${r},${g},${b},${a})`;
};
/** Mischt `a` zu Anteil `t` in `b` (beides #rrggbb). */
export const mix = (a, b, t) => {
  const A = hex2rgb(a), B = hex2rgb(b);
  const c = A.map((v, i) => Math.round(v * t + B[i] * (1 - t)));
  return '#' + c.map((v) => v.toString(16).padStart(2, '0')).join('');
};
const lin = (v) => { v /= 255; return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; };
export const luminance = (hex) => { const [r, g, b] = hex2rgb(hex); return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b); };
/** WCAG-Kontrast zweier #rrggbb-Farben. */
export const contrast = (a, b) => {
  const [x, y] = [luminance(a), luminance(b)].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
};

// ---------- Lucide-Icons aus dem Repo (assets/icons, ISC) ----------
const iconCache = new Map();
export function iconInner(name) {
  if (!iconCache.has(name)) {
    const src = readFileSync(join(ICON_DIR, `${name}.svg`), 'utf8');
    iconCache.set(name, src.replace(/^[\s\S]*?<svg[^>]*>/, '').replace(/<\/svg>\s*$/, ''));
  }
  return iconCache.get(name);
}
/** Icon als eigenständiges <svg> für HTML. */
export function iconSvg(name, size, color, extra = '') {
  return `<svg ${extra} width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="${color}" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${iconInner(name)}</svg>`;
}
/** Icon als Gruppe in einer SVG-Szene. */
export function ico(name, x, y, size, color) {
  return `<g transform="translate(${x} ${y}) scale(${size / 24})" fill="none" stroke="${color}" stroke-width="${(1.75 * 24 / size).toFixed(2)}" stroke-linecap="round" stroke-linejoin="round">${iconInner(name)}</g>`;
}

// ---------- SVG-Zeichner ----------
let uid = 0;
export const nextId = (p = 'u') => `${p}${++uid}`;
export const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

export const rect = (x, y, w, h, o = {}) =>
  `<rect x="${x}" y="${y}" width="${w}" height="${h}" rx="${o.r ?? 4}" fill="${o.fill ?? 'none'}"${o.stroke ? ` stroke="${o.stroke}" stroke-width="${o.sw ?? 1}"` : ''}${o.op != null ? ` opacity="${o.op}"` : ''}${o.filter ? ` filter="url(#${o.filter})"` : ''}${o.dash ? ` stroke-dasharray="${o.dash}"` : ''}/>`;
export const circ = (cx, cy, r, o = {}) =>
  `<circle cx="${cx}" cy="${cy}" r="${r}" fill="${o.fill ?? 'none'}"${o.stroke ? ` stroke="${o.stroke}" stroke-width="${o.sw ?? 1}"` : ''}${o.op != null ? ` opacity="${o.op}"` : ''}/>`;
export const line = (x1, y1, x2, y2, o = {}) =>
  `<line x1="${x1}" y1="${y1}" x2="${x2}" y2="${y2}" stroke="${o.stroke}" stroke-width="${o.sw ?? 1}"${o.dash ? ` stroke-dasharray="${o.dash}"` : ''}${o.op != null ? ` opacity="${o.op}"` : ''} stroke-linecap="round"/>`;
export const path = (d, o = {}) =>
  `<path d="${d}" fill="${o.fill ?? 'none'}"${o.stroke ? ` stroke="${o.stroke}" stroke-width="${o.sw ?? 1}" stroke-linecap="round" stroke-linejoin="round"` : ''}${o.op != null ? ` opacity="${o.op}"` : ''}/>`;
/** Text-Platzhalter: eine Linie. Stufe 1 = kräftig (Titel), 2 = Fließtext, 3 = leise. */
export const ln = (T, x, y, w, lvl = 2, h = 5) =>
  rect(x, y, w, h, { r: h / 2, fill: lvl === 1 ? T.text : lvl === 2 ? T.text2 : T.text3, op: lvl === 1 ? 0.78 : lvl === 2 ? 0.42 : 0.3 });
/** Fläche mit Rand (Fenster, Karte, Menü). */
export const panel = (T, x, y, w, h, o = {}) =>
  rect(x, y, w, h, { r: o.r ?? 4, fill: o.fill ?? T.panel, stroke: o.stroke ?? T.lineStrong, filter: o.filter });
/** Gefüllter Punkt / Avatar mit Buchstaben als Strichlinie (keine echte Schrift im SVG). */
export const dot = (cx, cy, r, fill, op) => circ(cx, cy, r, { fill, op });
export const pill = (T, x, y, w, h, fill, o = {}) => rect(x, y, w, h, { r: h / 2, fill, stroke: o.stroke, op: o.op });
/** Text im SVG (Inter, aus dem Seitenstil). */
export const txt = (x, y, s, o = {}) =>
  `<text x="${x}" y="${y}" font-family="Inter, system-ui, sans-serif" font-size="${o.size ?? 11}" font-weight="${o.w ?? 500}" fill="${o.fill}"${o.anchor ? ` text-anchor="${o.anchor}"` : ''}${o.op != null ? ` opacity="${o.op}"` : ''}>${esc(s)}</text>`;

/** Gemeinsame Defs einer Szene: Schatten, Glow, Punktraster. */
export function defs(T, id, hue) {
  return `<defs>
<filter id="sh-${id}" x="-20%" y="-20%" width="140%" height="150%"><feDropShadow dx="0" dy="6" stdDeviation="8" flood-color="${T.shadow}" flood-opacity="1"/></filter>
<filter id="sm-${id}" x="-20%" y="-20%" width="140%" height="150%"><feDropShadow dx="0" dy="2" stdDeviation="3" flood-color="${T.shadow}" flood-opacity=".8"/></filter>
<radialGradient id="gl-${id}" cx="78%" cy="8%" r="85%"><stop offset="0" stop-color="${hue}" stop-opacity="${T.name === 'dark' ? 0.34 : 0.26}"/><stop offset="1" stop-color="${hue}" stop-opacity="0"/></radialGradient>
<radialGradient id="g2-${id}" cx="10%" cy="105%" r="70%"><stop offset="0" stop-color="${hue}" stop-opacity="${T.name === 'dark' ? 0.16 : 0.14}"/><stop offset="1" stop-color="${hue}" stop-opacity="0"/></radialGradient>
<pattern id="dg-${id}" width="14" height="14" patternUnits="userSpaceOnUse"><circle cx="1" cy="1" r=".8" fill="${T.text3}" opacity="${T.name === 'dark' ? 0.30 : 0.28}"/></pattern>
<linearGradient id="fd-${id}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="${T.panel}" stop-opacity="0"/><stop offset="1" stop-color="${T.panel}" stop-opacity=".0"/></linearGradient>
</defs>`;
}
/** Grund der Bildfläche: getönte Fläche, Glühen in der Appfarbe, Punktraster. */
export function artGround(T, id, hue, w = 400, h = 200) {
  const base = mix(hue, T.panel, T.name === 'dark' ? 0.07 : 0.05);
  return `<rect width="${w}" height="${h}" fill="${base}"/><rect width="${w}" height="${h}" fill="url(#gl-${id})"/><rect width="${w}" height="${h}" fill="url(#g2-${id})"/><rect width="${w}" height="${h}" fill="url(#dg-${id})" opacity=".55"/>`;
}
