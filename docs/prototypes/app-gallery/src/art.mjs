// Bildmotive der Galerie-Studie (Issue #970): je App eine eigene, abstrahierte UI-Szene im Format 400 × 200.
// Gemeinsame Bildsprache: getönter Grund in der Appfarbe mit Glühen und Punktraster, darauf Flächen im Stil der
// echten Oberfläche (Fläche, Haarlinie, 4-px-Radius, Linear-Ruhe), Text nur als Linien, Farbe sparsam und nur
// in Marken. Alles ist handgezeichnetes SVG ohne fremde Vorlagen; Icons sind Lucide aus assets/icons.
import { THEMES, mix, rgba, rect, circ, line, path, ln, panel, dot, pill, txt, ico, defs, artGround, nextId } from './kit.mjs';
import { byId } from './apps.mjs';

const PRIO = { low: '#E8D36A', mid: '#E8B93D', high: '#E8873D', urgent: '#F0645A' };

// ---- Kalender: Wochenraster, Termine als Farbbalken, Jetzt-Linie, „Up next“ ----
function calendar(T, h, id) {
  const o = [];
  o.push(panel(T, 22, 24, 300, 190, { filter: `sh-${id}` }));
  const x0 = 62, cw = 52;
  for (let i = 0; i < 5; i++) {
    const cx = x0 + i * cw;
    if (i) o.push(line(cx, 48, cx, 214, { stroke: T.line, sw: 1 }));
    o.push(ln(T, cx + 14, 33, 22, 3, 4));
  }
  o.push(circ(x0 + 2 * cw + 26, 37, 8.5, { fill: T.accent }), txt(x0 + 2 * cw + 26, 40.5, '21', { size: 9.5, w: 600, fill: '#fff', anchor: 'middle' }));
  for (let y = 62; y < 214; y += 30) { o.push(line(x0, y, x0 + 5 * cw, y, { stroke: T.line })); o.push(ln(T, 32, y - 2, 16, 3, 3)); }
  const ev = (c, i, y, hh, tl = 30) => {
    const col = T.cal[c], x = x0 + i * cw + 3;
    o.push(rect(x, y, cw - 6, hh, { r: 3, fill: rgba(col, T.tint + 0.06) }), rect(x, y, 3, hh, { r: 1.5, fill: col }), ln(T, x + 8, y + 6, tl, 1, 4), ln(T, x + 8, y + 15, tl - 8, 3, 3));
  };
  ev(4, 0, 70, 46); ev(0, 1, 96, 30, 24); ev(1, 2, 64, 34); ev(2, 2, 118, 42); ev(3, 3, 86, 58); ev(4, 4, 112, 34, 26); ev(0, 4, 156, 38, 28);
  o.push(line(x0, 108, x0 + 5 * cw, 108, { stroke: T.accent, sw: 1.5 }), circ(x0, 108, 3.5, { fill: T.accent }));
  // schwebende Karte „Up next“
  o.push(panel(T, 262, 46, 120, 96, { fill: T.menu, filter: `sh-${id}`, r: 4 }));
  o.push(ico('calendar-clock', 272, 56, 14, h), ln(T, 292, 58, 40, 1, 5), ln(T, 292, 67, 28, 3, 4));
  o.push(rect(272, 82, 3, 22, { r: 1.5, fill: T.cal[0] }), ln(T, 281, 84, 60, 1, 5), ln(T, 281, 95, 40, 3, 4));
  o.push(ln(T, 272, 116, 46, 3, 4), pill(T, 326, 112, 46, 16, rgba(h, 0.9)), ln({ ...T, text: '#fff' }, 334, 118, 30, 1, 4));
  return o.join('');
}

// ---- Aufgaben: Zeilen mit Kreis, Fokus-Karte, Fortschritt, Fokus-Plätze ----
function tasks(T, h, id) {
  const o = [];
  o.push(panel(T, 30, 24, 262, 190, { filter: `sh-${id}` }));
  o.push(ln(T, 46, 40, 54, 1, 6), ln(T, 262, 41, 18, 3, 4));
  const row = (y, done, title, pr, chip) => {
    o.push(circ(52, y + 8, 7, done ? { fill: h } : { stroke: T.text3, sw: 1.5, op: 0.8 }));
    if (done) o.push(path(`M${48.5} ${y + 8} l2.4 2.4 l4.2 -4.6`, { stroke: '#fff', sw: 1.7 }));
    o.push(dot(70, y + 8, 3, PRIO[pr]), ln(T, 82, y + 5, title, done ? 3 : 1, 5), chip ? pill(T, 214, y + 2, 34, 12, rgba(chip, 0.2)) : '', chip ? dot(222, y + 8, 2.5, chip) : '', ln(T, 256, y + 5, 22, 3, 4));
    o.push(line(40, y + 26, 282, y + 26, { stroke: T.line }));
  };
  row(56, true, 64, 'low'); row(84, false, 92, 'mid', T.cal[0]);
  // starke Fokus-Karte
  o.push(rect(40, 114, 242, 58, { r: 4, fill: T.raised, stroke: T.line }));
  o.push(circ(52, 130, 10, { stroke: PRIO.high, sw: 3.5 }), ln(T, 72, 124, 120, 1, 6), ln(T, 214, 126, 30, 3, 4));
  o.push(ln(T, 72, 140, 40, 3, 4), ln(T, 118, 140, 80, 2, 4), rect(72, 152, 64, 4, { r: 2, fill: T.lineStrong }), rect(72, 152, 32, 4, { r: 2, fill: T.ok }), ln(T, 144, 151, 18, 3, 4));
  row(182, false, 70, 'urgent', T.cal[3]);
  // Karte „Today“ mit Fokus-Plätzen
  o.push(panel(T, 276, 40, 108, 70, { fill: T.menu, filter: `sh-${id}` }));
  o.push(ln(T, 288, 52, 36, 1, 5));
  [0, 1, 2].forEach((i) => o.push(circ(294 + i * 22, 76, 7, i < 2 ? { fill: h } : { stroke: T.text3, sw: 1.5 })));
  o.push(pill(T, 288, 92, 60, 12, T.hover), ln(T, 294, 96, 40, 2, 4));
  // Label-Chips
  o.push(pill(T, 296, 128, 56, 16, T.raised, { stroke: T.lineStrong }), dot(306, 136, 3, T.cal[1]), ln(T, 314, 134, 30, 2, 4));
  o.push(pill(T, 312, 150, 62, 16, T.raised, { stroke: T.lineStrong }), ico('github', 318, 152, 12, T.text2), ln(T, 334, 156, 34, 2, 4));
  return o.join('');
}

// ---- Mail: Liste mit Absender-Kreisen, Leser mit Kategorie-Pille, schwebende Aktionsleiste ----
function mail(T, h, id) {
  const o = [];
  o.push(panel(T, 20, 20, 196, 190, { filter: `sh-${id}` }));
  const rows = [[T.cal[3], 70, 100, true], [T.cal[1], 54, 112, false], [T.cal[0], 80, 84, false], [T.cal[4], 60, 106, false]];
  rows.forEach(([c, a, b, unread], i) => {
    const y = 28 + i * 44;
    if (i === 1) o.push(rect(21, y, 194, 44, { r: 0, fill: T.selected }), rect(21, y + 1, 2, 42, { r: 0, fill: T.accent }));
    o.push(circ(40, y + 22, 11, { fill: rgba(c, 0.9) }), txt(40, y + 26, ['AB', 'JK', 'MS', 'LW'][i], { size: 9, w: 600, fill: '#fff', anchor: 'middle' }));
    o.push(ln(T, 58, y + 13, a, 1, 5), ln(T, 58, y + 26, b, 3, 5), ln(T, 184, y + 14, 20, 3, 4));
    if (unread) o.push(dot(206, y + 28, 3, h));
    o.push(line(21, y + 44, 215, y + 44, { stroke: T.line }));
  });
  // Leser
  o.push(panel(T, 196, 38, 188, 172, { filter: `sh-${id}` }));
  o.push(circ(214, 60, 10, { fill: rgba(T.cal[1], 0.9) }), ln(T, 230, 54, 70, 1, 6), ln(T, 230, 66, 50, 3, 4));
  o.push(rect(328, 52, 46, 16, { r: 8, fill: h }), ico('inbox', 332, 55, 10, '#fff'), ln({ ...T, text: '#fff' }, 346, 58, 22, 1, 4));
  o.push(ln(T, 212, 90, 120, 1, 7));
  [0, 1, 2, 3, 4].forEach((i) => o.push(ln(T, 212, 108 + i * 12, i === 4 ? 70 : 140 - (i % 2) * 14, 2, 4)));
  o.push(rect(212, 176, 76, 22, { r: 4, fill: T.raised, stroke: T.lineStrong }), ico('paperclip', 218, 181, 12, T.text3), ln(T, 234, 185, 44, 2, 4));
  // schwebende Aktionsleiste
  o.push(panel(T, 354, 84, 36, 96, { fill: T.menu, r: 4, filter: `sh-${id}` }));
  o.push(rect(358, 90, 28, 22, { r: 4, fill: T.accent }), ico('reply', 364, 93, 16, '#fff'));
  o.push(ico('check', 364, 120, 16, T.text2), ico('flag', 364, 142, 16, T.text2), ico('save', 364, 162, 16, T.text2));
  return o.join('');
}

// ---- Notification Hub: gestapelte Entscheidungskarten ----
function hub(T, h, id) {
  const o = [];
  const card = (x, y, w, hh, f) => { o.push(rect(x, y, w, hh, { r: 4, fill: T.raised, stroke: T.lineStrong, filter: `sm-${id}` })); f(x, y); };
  card(58, 18, 264, 66, (x, y) => {
    o.push(rect(x + 12, y + 12, 28, 28, { r: 4, fill: rgba(T.attention, T.tint + 0.04) }), ico('shield-alert', x + 18, y + 18, 16, T.attention));
    o.push(ln(T, x + 50, y + 14, 110, 1, 6), ln(T, x + 50, y + 26, 150, 3, 4), ln(T, x + 218, y + 15, 26, 3, 4));
    o.push(rect(x + 50, y + 42, 50, 16, { r: 4, fill: T.panel, stroke: T.lineStrong }), ln(T, x + 60, y + 48, 30, 1, 4));
    o.push(ln(T, x + 112, y + 48, 26, 3, 4));
  });
  card(86, 94, 264, 66, (x, y) => {
    o.push(rect(x + 12, y + 12, 28, 28, { r: 4, fill: rgba(h, T.tint + 0.04) }), ico('bot', x + 18, y + 18, 16, h));
    o.push(path(`M${x + 26} ${y + 8} a18 18 0 0 1 18 18`, { stroke: h, sw: 2 }));
    o.push(ln(T, x + 50, y + 14, 100, 1, 6), ln(T, x + 50, y + 26, 130, 3, 4), ln(T, x + 218, y + 15, 26, 3, 4));
    o.push(rect(x + 50, y + 42, 44, 16, { r: 4, fill: T.panel, stroke: T.lineStrong }), ln(T, x + 58, y + 48, 28, 1, 4));
    o.push(rect(x + 100, y + 42, 54, 16, { r: 4, fill: T.panel, stroke: T.lineStrong }), ln(T, x + 108, y + 48, 38, 1, 4));
  });
  card(28, 140, 264, 62, (x, y) => {
    o.push(rect(x + 12, y + 12, 28, 28, { r: 4, fill: T.selected, stroke: T.lineStrong }), ico('cloud', x + 18, y + 18, 16, T.text));
    o.push(dot(x + 38, y + 14, 4, T.error));
    o.push(ln(T, x + 50, y + 14, 120, 1, 6), ln(T, x + 50, y + 26, 90, 3, 4), rect(x + 190, y + 18, 56, 18, { r: 4, fill: T.panel, stroke: T.lineStrong }), ln(T, x + 198, y + 25, 40, 1, 4));
  });
  // Zähler und Undo-Hinweis
  o.push(rect(336, 18, 34, 18, { r: 9, fill: h }), ln({ ...T, text: '#fff' }, 348, 24, 10, 1, 5));
  o.push(panel(T, 276, 170, 108, 26, { fill: T.menu, filter: `sm-${id}` }), ln(T, 288, 181, 36, 2, 5), ln(T, 346, 181, 22, 1, 5));
  return o.join('');
}

// ---- App Feed: ruhiger Strom mit Quellen-Kacheln ----
function feed(T, h, id) {
  const o = [];
  o.push(panel(T, 44, 18, 312, 196, { filter: `sh-${id}` }));
  o.push(ln(T, 60, 32, 32, 3, 4));
  const rows = [['github', 70, 130, false], ['cloud', 60, 150, false], ['message-circle', 84, 110, true], ['radio-tower', 66, 140, false], ['app-window', 74, 120, false]];
  rows.forEach(([icon, a, b, hot], i) => {
    const y = 46 + i * 34;
    if (i === 1) o.push(rect(45, y - 3, 310, 34, { r: 0, fill: T.hover }));
    const urgent = icon === 'radio-tower';
    o.push(rect(60, y + 2, 22, 22, { r: 4, fill: urgent ? rgba(T.attention, T.tint + 0.04) : T.selected, stroke: urgent ? undefined : T.lineStrong }), ico(icon, 64, y + 6, 14, urgent ? T.attention : T.text));
    o.push(ln(T, 94, y + 5, a, 1, 5), ln(T, 94, y + 16, b, 3, 4), ln(T, 322, y + 7, 22, 3, 4));
    if (hot) o.push(dot(94 + a + 10, y + 7.5, 3, h));
    if (urgent) o.push(pill(T, 94 + a + 8, y + 3, 40, 12, rgba(T.attention, 0.2)), ln(T, 94 + a + 14, y + 7, 28, 1, 4));
    o.push(line(60, y + 31, 340, y + 31, { stroke: T.line }));
  });
  // Aktionsleiste beim Überfahren der markierten Zeile
  o.push(panel(T, 276, 62, 74, 22, { fill: T.menu, r: 4, filter: `sm-${id}` }), ico('check', 284, 66, 14, T.text2), ico('bell-off', 304, 66, 14, T.text2), ico('external-link', 324, 66, 14, T.text2));
  return o.join('');
}

// ---- Assistent: Gespräch mit Werkzeug-Karte, Eingabefeld, Funken ----
function assistant(T, h, id) {
  const o = [];
  const star = (cx, cy, r, op = 1) => path(`M${cx} ${cy - r} Q${cx} ${cy} ${cx + r} ${cy} Q${cx} ${cy} ${cx} ${cy + r} Q${cx} ${cy} ${cx - r} ${cy} Q${cx} ${cy} ${cx} ${cy - r}Z`, { fill: h, op });
  o.push(star(34, 40, 14, 0.9), star(54, 70, 7, 0.6), star(360, 150, 11, 0.7), star(346, 176, 5, 0.5));
  o.push(rect(190, 22, 150, 30, { r: 4, fill: T.selected, stroke: T.lineStrong }), ln(T, 204, 32, 110, 1, 5), ln(T, 204, 42, 70, 3, 4));
  o.push(rect(76, 62, 22, 22, { r: 4, fill: rgba(h, T.tint + 0.04) }), ico('sparkles', 80, 66, 14, h));
  [0, 1, 2].forEach((i) => o.push(ln(T, 108, 66 + i * 11, i === 2 ? 70 : 150, i === 0 ? 1 : 2, 5)));
  o.push(rect(108, 104, 214, 34, { r: 4, fill: T.raised, stroke: T.lineStrong, filter: `sm-${id}` }), ico('calendar', 118, 113, 16, h), ln(T, 142, 115, 78, 1, 5), ln(T, 142, 125, 52, 3, 4), ico('circle-check', 298, 113, 16, T.ok));
  [0, 1].forEach((i) => o.push(ln(T, 108, 150 + i * 11, i ? 90 : 160, i ? 3 : 2, 5)));
  o.push(panel(T, 76, 168, 252, 28, { fill: T.menu, filter: `sm-${id}` }), ln(T, 90, 180, 110, 3, 5), ico('mic', 276, 174, 16, T.text2), rect(300, 173, 20, 18, { r: 4, fill: h }), ico('arrow-up', 303, 174, 14, '#fff'));
  return o.join('');
}

// ---- Chat: Kontaktliste mit Status, Unterhaltung, Anruf-Karte ----
function chat(T, h, id) {
  const o = [];
  o.push(panel(T, 22, 22, 124, 188, { filter: `sh-${id}` }));
  const st = [T.ok, T.attention, T.ok, T.text3, T.ok, T.text3];
  st.forEach((c, i) => {
    const y = 32 + i * 28;
    if (i === 0) o.push(rect(23, y - 4, 122, 28, { r: 0, fill: T.selected }));
    o.push(circ(40, y + 10, 9, { fill: rgba(T.cal[i % 5], 0.85) }), circ(47, y + 17, 3.5, { fill: c, stroke: T.panel, sw: 1.5 }), ln(T, 56, y + 4, 40 + (i % 3) * 10, i === 0 ? 1 : 2, 5), ln(T, 56, y + 15, 30, 3, 4));
  });
  o.push(panel(T, 156, 44, 226, 166, { filter: `sh-${id}` }));
  o.push(rect(170, 60, 110, 30, { r: 4, fill: T.raised, stroke: T.line }), ln(T, 180, 69, 80, 1, 5), ln(T, 180, 79, 52, 3, 4));
  o.push(rect(236, 100, 132, 30, { r: 4, fill: rgba(h, T.tint + 0.08), stroke: rgba(h, 0.35) }), ln(T, 246, 109, 100, 1, 5), ln(T, 246, 119, 60, 2, 4));
  o.push(rect(170, 140, 70, 32, { r: 4, fill: T.raised, stroke: T.line }), rect(176, 146, 58, 20, { r: 3, fill: rgba(T.cal[0], 0.3) }), path('M178 164 l14 -10 l12 7 l10 -6 l16 11', { stroke: rgba(T.cal[0], 0.9), sw: 1.4 }));
  [0, 1, 2].forEach((i) => o.push(circ(180 + i * 9, 188, 2.5, { fill: T.text3, op: 0.45 + i * 0.2 })));
  o.push(rect(300, 178, 70, 22, { r: 4, fill: T.menu, stroke: T.lineStrong }), ln(T, 310, 187, 40, 3, 4));
  // Anruf-Karte
  o.push(panel(T, 296, 14, 92, 34, { fill: T.menu, filter: `sm-${id}` }), rect(302, 21, 20, 20, { r: 10, fill: rgba(T.ok, 0.2) }), ico('phone', 306, 25, 12, T.ok), ln(T, 328, 24, 50, 1, 4), ln(T, 328, 33, 34, 3, 4));
  return o.join('');
}

// ---- Kontakte: gruppierte Liste, Karte mit Foto, Karte ohne Schlüssel, Geburtstag ----
function contacts(T, h, id) {
  const o = [];
  o.push(panel(T, 20, 20, 160, 192, { filter: `sh-${id}` }));
  o.push(ln(T, 34, 32, 10, 3, 5));
  const ini = ['AB', 'AV', 'AD', 'BK'];
  ini.forEach((s, i) => {
    const y = 46 + i * 34 + (i === 3 ? 18 : 0);
    if (i === 3) o.push(ln(T, 34, y - 14, 10, 3, 5));
    if (i === 1) o.push(rect(21, y - 4, 158, 34, { r: 0, fill: T.selected }), rect(21, y - 3, 2, 32, { r: 0, fill: T.accent }));
    o.push(circ(46, y + 13, 12, { fill: rgba(T.cal[i % 5], 0.85) }), txt(46, y + 17, s, { size: 9, w: 600, fill: '#fff', anchor: 'middle' }), ln(T, 66, y + 7, 70 - i * 6, 1, 5), ln(T, 66, y + 19, 50, 3, 4));
  });
  o.push(panel(T, 172, 38, 214, 174, { filter: `sh-${id}` }));
  o.push(circ(206, 76, 22, { fill: rgba(T.cal[1], 0.9) }), txt(206, 81, 'AV', { size: 13, w: 600, fill: '#fff', anchor: 'middle' }));
  o.push(ln(T, 238, 62, 100, 1, 7), ln(T, 238, 76, 70, 3, 5), rect(316, 66, 54, 16, { r: 8, fill: rgba(h, T.tint + 0.06) }), ico('cake', 321, 69, 10, h), ln(T, 334, 72, 30, 2, 4));
  o.push(ico('mail', 188, 112, 14, T.text3), ln(T, 210, 115, 90, 2, 5), ico('phone', 188, 134, 14, T.text3), ln(T, 210, 137, 70, 2, 5));
  // Karte ohne Schlüssel: Straßennetz und Pin
  o.push(rect(188, 156, 182, 44, { r: 4, fill: T.raised, stroke: T.line }));
  o.push(path('M188 180 L250 170 L300 186 L370 168', { stroke: T.lineStrong, sw: 2 }), path('M232 156 L246 200', { stroke: T.lineStrong, sw: 2 }), path('M310 156 L296 200', { stroke: T.lineStrong, sw: 2 }), path('M188 192 L370 190', { stroke: T.line, sw: 5 }));
  o.push(ico('map-pin', 270, 160, 22, h), circ(281, 171, 3, { fill: T.panel }));
  return o.join('');
}

// ---- Workplace: Kachel-Aufteilung, Tray-Menü, Workspace-Vorlage ----
function workplace(T, h, id) {
  const o = [];
  const tile = (x, y, w, hh, col, f) => {
    o.push(rect(x, y, w, hh, { r: 4, fill: T.panel, stroke: T.lineStrong, filter: `sm-${id}` }), rect(x, y, w, 3, { r: 0, fill: col, op: 0.9 }));
    o.push(dot(x + 9, y + 12, 2.5, T.text3, 0.5), dot(x + 17, y + 12, 2.5, T.text3, 0.5), dot(x + 25, y + 12, 2.5, T.text3, 0.5));
    f(x, y);
  };
  tile(22, 20, 140, 184, byId.calendar.hue[T.name], (x, y) => {
    [0, 1, 2, 3].forEach((i) => { o.push(line(x + 10, y + 34 + i * 34, x + 130, y + 34 + i * 34, { stroke: T.line })); });
    o.push(rect(x + 12, y + 44, 56, 30, { r: 3, fill: rgba(byId.calendar.hue[T.name], T.tint + 0.06) }), rect(x + 12, y + 44, 3, 30, { r: 1.5, fill: byId.calendar.hue[T.name] }), ln(T, x + 22, y + 52, 36, 1, 4));
    o.push(rect(x + 70, y + 94, 56, 40, { r: 3, fill: rgba(T.cal[0], T.tint + 0.06) }), rect(x + 70, y + 94, 3, 40, { r: 1.5, fill: T.cal[0] }), ln(T, x + 80, y + 102, 36, 1, 4));
  });
  tile(168, 20, 152, 86, byId.mail.hue[T.name], (x, y) => {
    [0, 1].forEach((i) => { o.push(circ(x + 16, y + 34 + i * 26, 7, { fill: rgba(T.cal[i + 1], 0.85) }), ln(T, x + 30, y + 30 + i * 26, 70, 1, 4), ln(T, x + 30, y + 40 + i * 26, 96, 3, 4)); });
  });
  tile(168, 112, 152, 92, byId.tasks.hue[T.name], (x, y) => {
    [0, 1, 2].forEach((i) => { o.push(circ(x + 16, y + 32 + i * 18, 5, i === 0 ? { fill: byId.tasks.hue[T.name] } : { stroke: T.text3, sw: 1.4 }), ln(T, x + 28, y + 30 + i * 18, 80 - i * 14, i === 0 ? 3 : 1, 4)); });
  });
  // Tray-Menü
  o.push(panel(T, 268, 8, 120, 100, { fill: T.menu, filter: `sh-${id}` }));
  o.push(rect(276, 16, 16, 16, { r: 4, fill: T.accent }), ico('layout-grid', 279, 19, 10, '#fff'), ln(T, 298, 21, 60, 1, 5));
  o.push(line(274, 40, 382, 40, { stroke: T.line }));
  ['Morning', 'Deep work', 'Support'].forEach((_, i) => { o.push(ico('layers', 278, 48 + i * 18, 12, T.text2), ln(T, 296, 52 + i * 18, 44 + i * 6, 2, 4), rect(350, 48 + i * 18, 24, 12, { r: 3, fill: T.hover, stroke: T.line }), ln(T, 355, 52 + i * 18, 14, 3, 4)); });
  return o.join('');
}

// ---- Notizen: Liste, Markdown-Editor mit Checkliste und Tags ----
function notes(T, h, id) {
  const o = [];
  o.push(panel(T, 20, 20, 104, 192, { filter: `sh-${id}` }));
  [0, 1, 2, 3].forEach((i) => {
    const y = 30 + i * 44;
    if (i === 1) o.push(rect(21, y - 4, 102, 42, { r: 0, fill: T.selected }));
    o.push(ln(T, 32, y + 2, 56 - i * 4, 1, 5), ln(T, 32, y + 14, 70, 3, 4), ln(T, 32, y + 24, 44, 3, 4));
  });
  o.push(panel(T, 112, 34, 272, 178, { filter: `sh-${id}` }));
  o.push(rect(130, 50, 120, 9, { r: 4, fill: T.text, op: 0.8 }), ln(T, 130, 70, 150, 2, 4));
  o.push(rect(130, 86, 22, 12, { r: 6, fill: rgba(h, T.tint + 0.08) }), ln(T, 136, 90, 10, 1, 4), rect(156, 86, 28, 12, { r: 6, fill: rgba(h, T.tint + 0.08) }), ln(T, 162, 90, 16, 1, 4));
  [['x', 120], ['x', 96], ['', 132]].forEach(([c, w], i) => {
    const y = 112 + i * 18;
    o.push(rect(130, y, 11, 11, { r: 3, fill: c ? h : 'none', stroke: c ? undefined : T.text3, sw: 1.3 }));
    if (c) o.push(path(`M${132.5} ${y + 5.5} l2.2 2.2 l3.6 -4.2`, { stroke: '#fff', sw: 1.6 }));
    o.push(ln(T, 150, y + 3, w, c ? 3 : 1, 5));
  });
  o.push(rect(130, 168, 236, 34, { r: 4, fill: T.raised, stroke: T.line }), ln(T, 142, 178, 90, 2, 4), ln(T, 142, 188, 130, 3, 4));
  return o.join('');
}

// ---- Fotos: Zeitleiste als Mauerwerk aus abstrakten Aufnahmen ----
export function photoTile(T, id, x, y, w, hh, a, b, shape = 0) {
  const gid = nextId('pt');
  const sun = shape % 2 === 0;
  return `<defs><linearGradient id="${gid}" x1="0" y1="0" x2="0.4" y2="1"><stop offset="0" stop-color="${a}"/><stop offset="1" stop-color="${b}"/></linearGradient><clipPath id="c${gid}"><rect x="${x}" y="${y}" width="${w}" height="${hh}" rx="3"/></clipPath></defs>
<g clip-path="url(#c${gid})"><rect x="${x}" y="${y}" width="${w}" height="${hh}" fill="url(#${gid})"/>
${sun ? circ(x + w * 0.7, y + hh * 0.32, Math.min(w, hh) * 0.13, { fill: '#fff', op: 0.75 }) : ''}
${path(`M${x} ${y + hh} L${x} ${y + hh * 0.72} L${x + w * 0.3} ${y + hh * 0.5} L${x + w * 0.55} ${y + hh * 0.72} L${x + w * 0.78} ${y + hh * 0.58} L${x + w} ${y + hh * 0.78} L${x + w} ${y + hh}Z`, { fill: '#0b0c10', op: 0.38 })}
${path(`M${x} ${y + hh} L${x} ${y + hh * 0.86} L${x + w * 0.4} ${y + hh * 0.74} L${x + w * 0.7} ${y + hh * 0.9} L${x + w} ${y + hh * 0.8} L${x + w} ${y + hh}Z`, { fill: '#0b0c10', op: 0.5 })}</g>
<rect x="${x}" y="${y}" width="${w}" height="${hh}" rx="3" fill="none" stroke="${T.lineStrong}"/>`;
}
const PH = [['#F0A6FB', '#7C3AED'], ['#FDBA74', '#BE185D'], ['#67E8F9', '#1D4ED8'], ['#86EFAC', '#0F766E'], ['#FDE68A', '#C2410C'], ['#A5B4FC', '#312E81']];
function photos(T, h, id) {
  const o = [];
  o.push(panel(T, 20, 16, 360, 198, { filter: `sh-${id}` }));
  o.push(ln(T, 36, 30, 60, 1, 6), ln(T, 104, 31, 30, 3, 4));
  const cols = [[36, 104], [152, 104], [268, 104]];
  const spec = [
    [[36, 44, 104, 62, 0], [36, 112, 104, 84, 1]],
    [[148, 44, 104, 86, 2], [148, 136, 104, 60, 3]],
    [[260, 44, 104, 50, 4], [260, 100, 104, 96, 5]],
  ];
  spec.forEach((col) => col.forEach(([x, y, w, hh, k]) => o.push(photoTile(T, id, x, y, w, hh, ...PH[k], k))));
  o.push(rect(336, 50, 22, 16, { r: 8, fill: 'rgba(0,0,0,.45)' }), ico('star', 341, 53, 10, '#fff'));
  o.push(rect(160, 176, 54, 16, { r: 8, fill: 'rgba(0,0,0,.5)' }), ico('map-pin', 166, 179, 10, '#fff'), ln({ ...T, text2: '#fff' }, 180, 183, 28, 2, 4));
  return o.join('');
}

// ---- Rahmen: ein Bild mit Passepartout auf einer Kachel, Punkte der Diashow ----
function frame(T, h, id) {
  const o = [];
  o.push(rect(30, 30, 62, 78, { r: 4, fill: T.panel, stroke: T.line, op: 0.7 }), rect(30, 120, 62, 80, { r: 4, fill: T.panel, stroke: T.line, op: 0.7 }));
  o.push(ln(T, 40, 44, 34, 3, 4), ln(T, 40, 56, 40, 3, 4), ln(T, 40, 136, 30, 3, 4));
  o.push(rect(104, 12, 270, 186, { r: 4, fill: T.panel, stroke: T.lineStrong, filter: `sh-${id}` }));
  o.push(rect(118, 26, 242, 142, { r: 2, fill: T.raised, stroke: T.line }));
  o.push(photoTile(T, id, 134, 40, 210, 114, '#FDBA74', '#BE185D', 0));
  [0, 1, 2, 3, 4].forEach((i) => o.push(circ(210 + i * 12, 184, i === 1 ? 3.2 : 2.4, { fill: i === 1 ? h : T.text3, op: i === 1 ? 1 : 0.5 })));
  o.push(ln(T, 134, 178, 40, 3, 4));
  return o.join('');
}

// ---- Agenten: Sitzungen mit Zustand, Verbrauch als Säulen ----
function agents(T, h, id) {
  const o = [];
  o.push(panel(T, 20, 20, 232, 190, { filter: `sh-${id}` }));
  const rows = [['feature/970-gallery', 'wait', 120], ['fix/mail-badge', 'run', 92], ['docs/api', 'done', 70], ['test/contacts', 'run', 104]];
  rows.forEach(([b, s, w], i) => {
    const y = 32 + i * 44;
    if (s === 'wait') o.push(rect(21, y - 6, 230, 44, { r: 0, fill: rgba(T.attention, T.tint * 0.6) }), rect(21, y - 5, 2, 42, { fill: T.attention }));
    if (s === 'run') o.push(circ(40, y + 16, 8, { stroke: T.lineStrong, sw: 2 }), path(`M40 ${y + 8} a8 8 0 0 1 8 8`, { stroke: h, sw: 2 }));
    if (s === 'wait') o.push(circ(40, y + 16, 8, { stroke: T.attention, sw: 2 }), dot(40, y + 16, 3, T.attention));
    if (s === 'done') o.push(ico('circle-check', 32, y + 8, 16, T.ok));
    o.push(txt(58, y + 14, b, { size: 10.5, w: 500, fill: T.text }), ln(T, 58, y + 24, w, 3, 4), ln(T, 214, y + 12, 26, 3, 4));
    o.push(line(21, y + 38, 251, y + 38, { stroke: T.line }));
  });
  o.push(panel(T, 262, 34, 122, 110, { fill: T.raised, filter: `sm-${id}` }));
  o.push(ln(T, 274, 46, 50, 1, 5));
  const ch = ['#3B78D8', '#C9701E', '#2E9E6B', '#B5509A'];
  [34, 52, 28, 66, 44, 74, 40].forEach((v, i) => {
    o.push(rect(276 + i * 15, 128 - v * 0.9, 10, v * 0.9, { r: 2, fill: ch[i % 2 ? 1 : 0], op: 0.9 }), rect(276 + i * 15, 128 - v * 0.9 * 0.35, 10, v * 0.9 * 0.35, { r: 2, fill: ch[2], op: 0.9 }));
  });
  o.push(line(272, 128, 376, 128, { stroke: T.lineStrong }));
  o.push(pill(T, 262, 154, 84, 18, rgba(T.attention, 0.2)), dot(272, 163, 3, T.attention), ln(T, 280, 160, 56, 1, 4));
  return o.join('');
}

// ---- GitHub: Pull-Request-Zeilen mit Prüfungen ----
function github(T, h, id) {
  const merged = T.name === 'dark' ? '#A78BFA' : '#7C5CE6';
  const o = [];
  o.push(panel(T, 28, 20, 344, 192, { filter: `sh-${id}` }));
  const rows = [['git-pull-request', T.ok, 'circle-dot-dashed', T.attention, 'eye'], ['git-pull-request', T.ok, 'circle-check', T.ok, ''], ['git-pull-request-draft', T.text3, 'circle-dashed', T.text3, ''], ['git-merge', merged, 'circle-check', T.ok, ''], ['git-pull-request-closed', T.error, 'circle-x', T.error, '']];
  rows.forEach(([ic, c, ck, cc, extra], i) => {
    const y = 32 + i * 35;
    if (i === 0) o.push(rect(29, y - 4, 342, 35, { r: 0, fill: T.selected }), rect(29, y - 3, 2, 33, { fill: T.accent }));
    o.push(ico(ic, 44, y + 4, 16, c), ln(T, 70, y + 4, 110 - (i % 3) * 14, 1, 5), ln(T, 70, y + 16, 70, 3, 4));
    o.push(pill(T, 224, y + 4, 46, 14, T.raised, { stroke: T.lineStrong }), dot(234, y + 11, 3, T.cal[(i + 1) % 5]), ln(T, 242, y + 9, 22, 2, 4));
    if (extra) o.push(ico(extra, 280, y + 2, 14, T.attention));
    o.push(ico(ck, 330, y + 3, 16, cc), ln(T, 296, y + 8, 22, 3, 4));
    o.push(line(29, y + 31, 371, y + 31, { stroke: T.line }));
  });
  return o.join('');
}

// ---- Pinboard: ruhiges Raster mit angehefteten Zetteln ----
function pinboard(T, h, id) {
  const o = [];
  o.push(rect(14, 14, 372, 186, { r: 4, fill: T.panel, stroke: T.lineStrong, filter: `sh-${id}` }), rect(14, 14, 372, 186, { r: 4, fill: `url(#dg-${id})`, op: 0.9 }));
  const pin = (x, y, w, hh, col, f) => { o.push(rect(x, y, w, hh, { r: 4, fill: T.raised, stroke: T.lineStrong, filter: `sm-${id}` }), rect(x, y, w, 3, { r: 0, fill: col })); o.push(dot(x + w - 10, y + 12, 3.5, col, 0.9)); f(x, y); };
  pin(32, 32, 108, 64, T.cal[2], (x, y) => { o.push(ln(T, x + 10, y + 16, 70, 1, 5), ln(T, x + 10, y + 28, 86, 3, 4), ln(T, x + 10, y + 38, 60, 3, 4)); });
  pin(152, 24, 110, 84, T.cal[1], (x, y) => { [0, 1, 2].forEach((i) => { o.push(rect(x + 10, y + 16 + i * 18, 10, 10, { r: 3, fill: i === 0 ? T.cal[1] : 'none', stroke: i === 0 ? undefined : T.text3, sw: 1.3 }), ln(T, x + 28, y + 18 + i * 18, 58 - i * 8, i === 0 ? 3 : 1, 5)); }); });
  pin(274, 36, 96, 56, T.cal[0], (x, y) => { o.push(ico('globe', x + 10, y + 14, 14, T.text2), ln(T, x + 30, y + 17, 50, 1, 5), ln(T, x + 10, y + 36, 74, 3, 4)); });
  pin(46, 112, 96, 70, h, (x, y) => { o.push(ico('file-text', x + 10, y + 14, 16, T.text2), ln(T, x + 32, y + 18, 44, 1, 5), pill(T, x + 10, y + 44, 44, 14, rgba(h, 0.2)), ln(T, x + 18, y + 49, 28, 1, 4)); });
  pin(160, 124, 112, 62, T.cal[3], (x, y) => { o.push(ln(T, x + 10, y + 16, 80, 1, 5), ln(T, x + 10, y + 28, 90, 3, 4), ln(T, x + 10, y + 38, 54, 3, 4)); });
  pin(284, 108, 86, 78, T.cal[4], (x, y) => { o.push(ln(T, x + 10, y + 16, 54, 1, 5), ln(T, x + 10, y + 28, 64, 3, 4), ico('bot', x + 10, y + 46, 16, T.text3), ln(T, x + 32, y + 52, 36, 3, 4)); });
  return o.join('');
}

export const SCENES = { calendar, tasks, mail, hub, feed, assistant, chat, contacts, workplace, notes, photos, frame, agents, github, pinboard };

/** Komplette Bildfläche einer App als <svg>; `w`/`h` sind CSS-Maße, die Zeichnung bleibt 400 × 200. */
export function artSvg(appId, T, { w = '100%', h = '100%', slice = false, view = [0, 0, 400, 200] } = {}) {
  const app = byId[appId];
  const hue = app.hue[T.name];
  const id = nextId('a');
  const scene = SCENES[appId];
  const [vx, vy, vw, vh] = view;
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="${vx} ${vy} ${vw} ${vh}" preserveAspectRatio="${slice ? 'xMidYMid slice' : 'xMidYMid meet'}" role="img" aria-label="${app.name}: Motiv">${defs(T, id, hue)}${artGround(T, id, hue)}${scene(T, hue, id)}</svg>`;
}
export { THEMES };
