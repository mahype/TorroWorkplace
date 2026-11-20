// Spike for issue #917. Squire 2.4.11 (squire-rte, MIT, assets/squire.js) as a WYSIWYG editor in the system
// webview. Everything the page learns goes to the Rust side as a JSON line (window.ipc.postMessage).
const post = (kind, data) => window.ipc.postMessage(JSON.stringify({ kind, ...data }));
const logEl = document.getElementById('log');
const log = (text) => { logEl.textContent += text + '\n'; logEl.scrollTop = logEl.scrollHeight; };
const ed = document.getElementById('editor');
const squire = new Squire(ed);
window.squire = squire;

// Images: the draft says `cid:x`, the webview needs an address it can load. `torro://img/x` is the
// custom protocol of the Rust side; saving turns it back.
const toView = (html) => html.replace(/src="cid:([^"]+)"/g, 'src="torro://img/$1"');
const toCid = (html) => html.replace(/src="torro:\/\/img\/([^"]+)"/g, 'src="cid:$1"');

document.querySelectorAll('#bar button').forEach((b) => b.addEventListener('click', () => {
  const c = b.dataset.cmd;
  if (c === 'bold') squire.bold();
  else if (c === 'italic') squire.italic();
  else if (c === 'list') squire.makeUnorderedList();
  else if (c === 'quote') squire.increaseQuoteLevel();
  else if (c === 'undo') squire.undo();
  else if (c === 'redo') squire.redo();
  squire.focus();
}));

// Does the point lie on text (not on the padding or the empty rest of the window)?
const onText = (x, y) => {
  const r = document.caretPositionFromPoint ? document.caretPositionFromPoint(x, y) : null;
  return !!(r && ed.contains(r.offsetNode));
};

// Drops that reach the page (the DOM side; the native side reports its own events).
for (const t of ['dragenter', 'dragover']) {
  document.addEventListener(t, (e) => { e.preventDefault(); ed.classList.add('over'); });
}
document.addEventListener('dragleave', () => ed.classList.remove('over'));
document.addEventListener('drop', (e) => {
  e.preventDefault();
  ed.classList.remove('over');
  const files = [...(e.dataTransfer ? e.dataTransfer.files : [])].map((f) => ({ name: f.name, type: f.type, size: f.size }));
  const types = e.dataTransfer ? [...e.dataTransfer.types] : [];
  const inText = onText(e.clientX, e.clientY);
  post('dom-drop', { x: e.clientX, y: e.clientY, inText, files, types });
  log('dom-drop ' + JSON.stringify({ inText, files, types }));
  if (inText) {
    // An image on the text: embed it at the spot, as a blob the page can show.
    for (const f of e.dataTransfer.files) {
      if (f.type.startsWith('image/')) {
        const pos = document.caretPositionFromPoint(e.clientX, e.clientY);
        if (pos) {
          const range = document.createRange();
          range.setStart(pos.offsetNode, pos.offset);
          squire.setSelection(range);
        }
        squire.insertImage(URL.createObjectURL(f), { alt: f.name });
      }
    }
  }
});

// Paste: which types were offered, and what Squire made of them.
squire.addEventListener('willPaste', (e) => {
  const types = e.clipboardData ? [...e.clipboardData.types] : [];
  post('paste', { types, fragment: e.fragment ? [...e.fragment.childNodes].map((n) => n.outerHTML || n.textContent).join('') : null });
  log('paste ' + types.join(','));
});
squire.addEventListener('input', () => post('input', { html: toCid(squire.getHTML()) }));
squire.addEventListener('undoStateChange', (e) => post('undo-state', { canUndo: e.canUndo, canRedo: e.canRedo }));

// Called from Rust: load the draft fixture, take it out again as a draft would be saved.
window.__load = (html) => squire.setHTML(toView(html));
window.__save = () => toCid(squire.getHTML());
window.__roundtrip = (html) => {
  window.__load(html);
  // Give the webview a moment to load the `torro://img/` pictures, then say how many drew.
  setTimeout(() => {
    const imgs = [...ed.querySelectorAll('img')];
    post('roundtrip', {
      html: window.__save(), text: ed.innerText,
      images: imgs.map((i) => ({ src: i.getAttribute('src'), width: i.naturalWidth })),
    });
  }, 500);
};
window.__text = () => post('text', { text: ed.innerText, html: window.__save() });

post('ready', { squire: typeof Squire, ua: navigator.userAgent });
squire.focus();
