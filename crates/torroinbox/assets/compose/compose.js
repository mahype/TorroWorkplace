// The compose window's page (issue #969). Squire 2.4.11 (squire.js, MIT) is the editor; DOMPurify (purify.min.js)
// cleans what Squire is handed. Everything the page learns goes to the program as one JSON line
// (`window.ipc.postMessage`), everything the program says arrives in `window.torroCompose.receive`. The page holds no
// secret and no rights: it shows what the session (crates/torroinbox/src/compose/session.rs) tells it and reports
// what the person did. Texts are English, asked for by their English words with T; the program sends their translation.
"use strict";

window.onerror = (message, source, line) => post({ t: "error", message: String(message) + " (" + line + ")" });

function post(message) { window.ipc.postMessage(JSON.stringify(message)); }
const $ = (selector, root = document) => root.querySelector(selector);
const $$ = (selector, root = document) => [...root.querySelectorAll(selector)];
let strings = {};
const T = (english) => strings[english] ?? english;
const MAC = /Mac OS X/.test(navigator.userAgent);
const MOD = MAC ? "Meta-" : "Ctrl-";

const state = {
  ready: false, answer: false, forward: false, history: false, senders: [], from: 0, images: {}, locked: false,
  paper: "light", writableBook: false, sendKeys: "", token: 0, pasteSeq: 0, inserts: {}, selected: null,
  sending: false, asking: null, plainNext: false,
};

// ---------------------------------------------------------------------------------------------
// The editor
// ---------------------------------------------------------------------------------------------

const editorEl = $("#editor");
const squire = new Squire(editorEl, {
  blockTag: "DIV",
  // What Squire is handed (a paste that came through the program, setHTML) is cleaned once more.
  sanitizeToDOMFragment: (html) => {
    const clean = window.DOMPurify.sanitize(html, {
      ALLOWED_TAGS: ["p", "div", "br", "hr", "blockquote", "ul", "ol", "li", "pre", "code", "h1", "h2", "h3", "h4", "b", "strong",
        "i", "em", "u", "s", "a", "img", "table", "thead", "tbody", "tr", "td", "th"],
      ALLOWED_ATTR: ["href", "src", "alt", "width", "height", "type", "class", "data-cid"],
      ALLOWED_URI_REGEXP: /^(?:https?:|mailto:|\/img\/|cid:)/i,
      RETURN_DOM_FRAGMENT: true,
    });
    return document.importNode(clean, true);
  },
});
window.squire = squire;

// The text as the draft keeps it. Squire drops the `type="cite"` of a quote; the quoted history of an answer (the
// element with the class torro-quote) is written with it, as `markup::to_html` does, so it stays what it was.
function editorHtml() {
  const html = squire.getHTML();
  if (!html.includes("torro-quote")) return html;
  const body = new DOMParser().parseFromString("<!doctype html><body>" + html, "text/html").body;
  body.querySelectorAll(".torro-quote blockquote").forEach((quote) => quote.setAttribute("type", "cite"));
  return body.innerHTML;
}

function setEditorHtml(html) {
  // An answer starts with room above the quote: one empty line to type on.
  if (/^\s*<div class="torro-quote"/.test(html)) html = "<div><br></div>" + html;
  squire.setHTML(html);
  refreshEmpty();
  applySizes();
}

// Squire keeps a picture's id but drops its width: the page shows each picture at the width of its size (the program
// writes the width into the draft from the size, not from the page).
function applySizes() {
  editorEl.querySelectorAll("img[data-cid]").forEach((img) => {
    const info = state.images[img.dataset.cid];
    const width = info ? (info.widths[info.size || "medium"] || info.width) + "px" : "";
    if (width && img.style.width !== width) img.style.width = width;
  });
}

function refreshEmpty() {
  const empty = !editorEl.textContent.trim() && !editorEl.querySelector("img");
  editorEl.classList.toggle("empty", empty);
}

// The shortcuts Squire has not: link and clear (send and Esc are the document's, they also work in the fields). Bold,
// italic, underline, undo and redo are Squire's own. Squire names a key by its modifiers and `event.key`.
squire.setKeyHandler(MOD + "k", (self, event) => { event.preventDefault(); openLinkBar(); });
squire.setKeyHandler(MOD + "\\", (self, event) => { event.preventDefault(); squire.removeAllFormatting(); });
squire.setKeyHandler("Shift-Tab", (self, event) => {
  // Out of a list item Shift+Tab goes to the subject; in one it takes the item a level out.
  if (squire.hasFormat("LI")) return;
  event.preventDefault();
  $("#subject").focus();
});

squire.addEventListener("input", () => { refreshEmpty(); applySizes(); changed(); });
squire.addEventListener("pathChange", () => { refreshTools(); });
squire.addEventListener("undoStateChange", (event) => {
  $("[data-cmd=undo]").disabled = !event.detail.canUndo;
  $("[data-cmd=redo]").disabled = !event.detail.canRedo;
});
squire.addEventListener("select", () => { refreshTools(); });

function refreshTools() {
  const on = (cmd, yes) => $(`[data-cmd=${cmd}]`).setAttribute("aria-pressed", yes ? "true" : "false");
  on("bold", squire.hasFormat("B"));
  on("italic", squire.hasFormat("I"));
  on("underline", squire.hasFormat("U"));
  on("ul", squire.hasFormat("UL"));
  on("ol", squire.hasFormat("OL"));
  on("quote", squire.hasFormat("BLOCKQUOTE"));
  on("link", squire.hasFormat("A"));
}

const commands = {
  bold: () => (squire.hasFormat("B") ? squire.removeBold() : squire.bold()),
  italic: () => (squire.hasFormat("I") ? squire.removeItalic() : squire.italic()),
  underline: () => (squire.hasFormat("U") ? squire.removeUnderline() : squire.underline()),
  ul: () => (squire.hasFormat("UL") ? squire.removeList() : squire.makeUnorderedList()),
  ol: () => (squire.hasFormat("OL") ? squire.removeList() : squire.makeOrderedList()),
  quote: () => (squire.hasFormat("BLOCKQUOTE") ? squire.decreaseQuoteLevel() : squire.increaseQuoteLevel()),
  link: () => openLinkBar(),
  clear: () => squire.removeAllFormatting(),
  undo: () => squire.undo(),
  redo: () => squire.redo(),
};
$$("[data-cmd]").forEach((button) => {
  // The buttons never take the focus from the text, so the selection stays.
  button.addEventListener("mousedown", (event) => event.preventDefault());
  button.addEventListener("click", () => { commands[button.dataset.cmd](); squire.focus(); refreshTools(); });
});

// ----- links -----

const linkBar = $("#linkbar");
const linkInput = $("#link-input");
let linkRange = null;

function openLinkBar() {
  hidePopovers();
  linkRange = squire.getSelection();
  const current = squire.hasFormat("A") ? (squire.getSelection().commonAncestorContainer.closest?.("a") || squire.getSelection().startContainer.parentElement?.closest("a")) : null;
  linkInput.value = current ? current.getAttribute("href") || "" : "";
  placeBelow(linkBar, $("[data-cmd=link]"));
  linkBar.hidden = false;
  linkInput.focus();
  linkInput.select();
}

// What the person typed as an address of a link: a web address, or a mail address; nothing else becomes a link.
function linkAddress(text) {
  const value = text.trim();
  if (!value) return null;
  if (/^(https?:|mailto:)/i.test(value)) return value;
  if (/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(value)) return "mailto:" + value;
  if (/^[^\s:/]+\.[^\s:/]+/.test(value) || value.startsWith("localhost")) return "https://" + value;
  return null;
}

function applyLink() {
  const address = linkAddress(linkInput.value);
  if (address && linkRange) {
    squire.focus();
    squire.setSelection(linkRange);
    squire.makeLink(address);
  }
  linkBar.hidden = true;
  squire.focus();
}
$("#link-apply").addEventListener("click", applyLink);
$("#link-remove").addEventListener("click", () => {
  squire.focus();
  if (linkRange) squire.setSelection(linkRange);
  squire.removeLink();
  linkBar.hidden = true;
});
linkInput.addEventListener("keydown", (event) => {
  if (event.key === "Enter") { event.preventDefault(); applyLink(); }
  if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); linkBar.hidden = true; squire.focus(); }
});

// A link in the text opens in the system's browser, with the modifier held; a plain click puts the caret in it.
editorEl.addEventListener("click", (event) => {
  const link = event.target.closest?.("a");
  if (link) {
    event.preventDefault();
    if (event.metaKey || event.ctrlKey) post({ t: "open_link", url: link.getAttribute("href") || "" });
  }
});

// ----- pictures -----

const imgBar = $("#imgbar");

function placeBelow(popover, anchor) {
  const rect = anchor.getBoundingClientRect();
  popover.hidden = false;
  const width = popover.offsetWidth;
  popover.style.left = Math.max(8, Math.min(rect.left, window.innerWidth - width - 8)) + "px";
  const below = rect.bottom + 6;
  popover.style.top = (below + popover.offsetHeight > window.innerHeight ? Math.max(8, rect.top - popover.offsetHeight - 6) : below) + "px";
}

function selectImage(img) {
  deselectImage();
  if (!img) return;
  state.selected = img;
  img.classList.add("sel");
  const info = state.images[img.dataset.cid] || {};
  $$("[data-size]", imgBar).forEach((button) => button.classList.toggle("cur", button.dataset.size === (info.size || "medium")));
  placeBelow(imgBar, img);
}
function deselectImage() {
  if (state.selected) state.selected.classList.remove("sel");
  state.selected = null;
  imgBar.hidden = true;
}
editorEl.addEventListener("click", (event) => {
  if (event.target.tagName === "IMG") selectImage(event.target);
  else deselectImage();
});
$$("[data-size]", imgBar).forEach((button) => button.addEventListener("click", () => {
  const img = state.selected;
  if (!img) return;
  const info = state.images[img.dataset.cid];
  if (!info) return;
  info.size = button.dataset.size;
  applySizes();
  post({ t: "image_size", cid: img.dataset.cid, size: info.size });
  selectImage(img);
}));
$("#img-remove").addEventListener("click", () => {
  const img = state.selected;
  if (!img) return;
  deselectImage();
  squire.modifyDocument(() => img.remove());
  changed();
});
$("#img-attach").addEventListener("click", () => {
  const img = state.selected;
  if (!img) return;
  const cid = img.dataset.cid;
  deselectImage();
  squire.modifyDocument(() => img.remove());
  post({ t: "image_attach", cid });
  changed();
});

// A picture from the program goes to the place the person asked for it (the caret when they asked).
function insertImage(token, image) {
  state.images[image.cid] = image;
  const range = state.inserts[token];
  delete state.inserts[token];
  squire.focus();
  if (range) squire.setSelection(range);
  squire.insertImage("/img/" + image.cid, { alt: image.name || "", "data-cid": image.cid });
  refreshEmpty();
  applySizes();
  changed();
}

function wantImage(range, send) {
  const token = ++state.token;
  state.inserts[token] = range || squire.getSelection();
  send(token);
}

$("#image").addEventListener("click", () => wantImage(squire.getSelection(), (token) => post({ t: "image_pick", token })));

// ----- paste -----

// The page does not take what the clipboard offers: it hands it to the program, which cleans it to what mail allows
// (`html::prepare_paste`) and answers with HTML for `insertHTML`. A picture on the clipboard goes the same way.
$("#paper").addEventListener("paste", (event) => {
  if (!editorEl.contains(event.target) && event.target !== editorEl) return;
  const data = event.clipboardData;
  if (!data) return;
  event.preventDefault();
  event.stopPropagation();
  const plain = state.plainNext;
  state.plainNext = false;
  const html = data.getData("text/html");
  const text = data.getData("text/plain");
  const image = [...data.files].find((file) => file.type.startsWith("image/"));
  if (image && !html && !text) {
    const token = ++state.token;
    state.inserts[token] = squire.getSelection();
    const reader = new FileReader();
    reader.onload = () => post({ t: "image_bytes", token, name: image.name || T("Image"), base64: String(reader.result).split(",")[1] || "" });
    reader.readAsDataURL(image);
    return;
  }
  const seq = ++state.pasteSeq;
  state.inserts["paste" + seq] = squire.getSelection();
  post({ t: "paste", seq, html: html || null, text: text || null, plain });
}, true);
document.addEventListener("keydown", (event) => {
  // The shortcut that keeps the text only; the paste event follows and finds the flag.
  state.plainNext = (event.metaKey || event.ctrlKey) && event.shiftKey && event.key.toLowerCase() === "v";
}, true);

function pasteResult(event) {
  const range = state.inserts["paste" + event.seq];
  delete state.inserts["paste" + event.seq];
  if (event.html == null) return;
  squire.focus();
  if (range) squire.setSelection(range);
  squire.insertHTML(event.html);
  refreshEmpty();
  changed();
}

// ---------------------------------------------------------------------------------------------
// Recipients
// ---------------------------------------------------------------------------------------------

const ICON_ALERT = '<svg class="alert" viewBox="0 0 24 24"><circle cx="12" cy="12" r="9"/><path d="M12 8v4M12 16h.01"/></svg>';
const ICON_X = '<svg viewBox="0 0 24 24"><path d="M6 6l12 12M18 6 6 18"/></svg>';
const ICON_USERS = '<svg viewBox="0 0 24 24"><circle cx="9" cy="8" r="4"/><path d="M2 21a7 7 0 0 1 14 0M16 3.1a4 4 0 0 1 0 7.8M22 21a7 7 0 0 0-4-6.3"/></svg>';
const ICON_MAIL = '<svg viewBox="0 0 24 24"><rect x="3" y="5" width="18" height="14" rx="2"/><path d="m3 7 9 6 9-6"/></svg>';
const SUGGEST_AFTER = 150;

function initials(name) {
  const words = name.trim().split(/\s+/).filter(Boolean);
  return ((words[0] || "?")[0] + (words.length > 1 ? words[words.length - 1][0] : "")).toUpperCase();
}

class Recipients {
  constructor(name) {
    this.name = name;
    this.row = $(`#row-${name}`);
    this.input = $("input", this.row);
    this.val = $(".val", this.row);
    this.box = $(".suggest", this.row);
    this.chips = [];
    this.items = [];
    this.lit = 0;
    this.seq = 0;
    this.asked = "";
    this.timer = null;
    this.problem = null;
    this.bind();
  }

  bind() {
    this.input.addEventListener("input", () => this.typed());
    this.input.addEventListener("focus", () => this.row.classList.add("focus"));
    this.input.addEventListener("blur", () => { this.row.classList.remove("focus"); this.commit(); this.close(); });
    this.input.addEventListener("keydown", (event) => this.key(event));
    this.row.addEventListener("click", (event) => { if (event.target === this.row || event.target === this.val) this.input.focus(); });
  }

  set({ chips, typed }) {
    this.chips = chips.slice();
    this.input.value = typed || "";
    this.render();
  }

  data() { return { chips: this.chips, typed: this.input.value }; }

  render() {
    $$(".chip", this.val).forEach((chip) => chip.remove());
    this.chips.forEach((chip, index) => {
      const el = document.createElement("span");
      el.className = "chip" + (chip.broken ? " bad" : "");
      el.title = chip.broken ? T("This isn't a valid address.") : chip.address;
      if (chip.broken) el.insertAdjacentHTML("beforeend", ICON_ALERT);
      const label = document.createElement("span");
      label.className = "name";
      label.textContent = chip.broken ? chip.address : (chip.name || chip.address);
      el.append(label);
      const x = document.createElement("button");
      x.className = "x";
      x.type = "button";
      x.title = T("Remove");
      x.innerHTML = ICON_X;
      x.addEventListener("mousedown", (event) => event.preventDefault());
      x.addEventListener("click", () => { this.chips.splice(index, 1); this.render(); changed(); this.input.focus(); });
      el.append(x);
      this.val.insertBefore(el, this.input);
    });
  }

  known(address) { return this.chips.some((chip) => !chip.broken && chip.address.toLowerCase() === address.toLowerCase()); }

  add(chip) {
    if (!chip.broken && this.known(chip.address)) return;
    this.chips.push({ address: chip.address, name: chip.name || null, broken: !!chip.broken });
    this.render();
    changed();
  }

  // The part of the typed text up to its last separator (`,` or `;` outside quotes and angle brackets) becomes chips.
  typed() {
    const value = this.input.value;
    let quoted = false, angle = false, last = -1;
    for (let i = 0; i < value.length; i++) {
      const c = value[i];
      if (c === '"') quoted = !quoted;
      else if (c === "<" && !quoted) angle = true;
      else if (c === ">" && !quoted) angle = false;
      else if ((c === "," || c === ";") && !quoted && !angle) last = i;
    }
    if (last >= 0) {
      this.input.value = value.slice(last + 1).trimStart();
      post({ t: "commit", field: this.name, text: value.slice(0, last) });
    }
    changed();
    this.ask();
  }

  commit() {
    const text = this.input.value.trim();
    this.input.value = "";
    if (text) post({ t: "commit", field: this.name, text });
    return !!text;
  }

  ask() {
    clearTimeout(this.timer);
    const query = this.input.value.trim();
    if (!query) { this.close(); return; }
    this.timer = setTimeout(() => {
      this.asked = query;
      post({ t: "suggest", field: this.name, seq: ++this.seq, query });
    }, SUGGEST_AFTER);
  }

  open() { return !this.box.hidden && this.items.length > 0; }
  close() { this.box.hidden = true; this.items = []; this.problem = null; }

  // The program answers with the suggestions for the keystroke `seq`; an answer to an older one is dropped.
  suggestions(seq, items) {
    if (seq !== this.seq || document.activeElement !== this.input) return;
    this.items = items;
    this.lit = 0;
    this.show();
  }

  show() {
    this.box.replaceChildren();
    if (!this.items.length) { this.box.hidden = true; return; }
    let section = null;
    this.items.forEach((item, index) => {
      const heading = item.kind === "seen" ? "seen" : "known";
      if (heading !== section) {
        section = heading;
        const h = document.createElement("div");
        h.className = "sg-h";
        h.textContent = heading === "seen" ? T("From mail, not in contacts") : T("Contacts");
        this.box.append(h);
      }
      this.box.append(this.row_(item, index));
    });
    if (this.problem) {
      const p = document.createElement("div");
      p.className = "sg-problem";
      p.textContent = this.problem;
      this.box.append(p);
    }
    this.box.hidden = false;
  }

  row_(item, index) {
    const el = document.createElement("div");
    el.className = "sg" + (index === this.lit ? " lit" : "");
    el.setAttribute("role", "option");
    const lead = document.createElement("span");
    lead.className = "av" + (item.kind === "contact" ? "" : " tile");
    if (item.kind === "group") lead.innerHTML = ICON_USERS;
    else if (item.kind === "seen") lead.innerHTML = ICON_MAIL;
    else { lead.textContent = initials(item.name); if (item.contact) lead.dataset.contact = item.contact; }
    const who = document.createElement("div");
    who.className = "who";
    const name = document.createElement("span");
    name.className = "n";
    name.textContent = item.name;
    who.append(name);
    if (item.detail) {
      const detail = document.createElement("span");
      detail.className = "d";
      detail.textContent = item.detail;
      who.append(detail);
    }
    el.append(lead, who);
    if (item.trail) { const trail = document.createElement("em"); trail.textContent = item.trail; el.append(trail); }
    if (item.addable && item.addresses[0]) {
      const add = document.createElement("button");
      add.type = "button";
      add.className = "add";
      add.textContent = "+ " + T("Contact");
      add.addEventListener("mousedown", (event) => { event.preventDefault(); event.stopPropagation(); });
      add.addEventListener("click", (event) => {
        event.stopPropagation();
        post({ t: "add_contact", name: item.addresses[0].name || "", address: item.addresses[0].address });
      });
      el.append(add);
    }
    // The mouse never takes the focus from the field: the list stays.
    el.addEventListener("mousedown", (event) => event.preventDefault());
    el.addEventListener("click", () => this.take(index));
    return el;
  }

  take(index) {
    const item = this.items[index];
    if (!item) return;
    const others = Object.values(fields);
    for (const who of item.addresses) {
      if (item.kind === "group" && others.some((field) => field.known(who.address))) continue;
      this.add({ address: who.address, name: who.name });
    }
    this.input.value = "";
    this.close();
    changed();
  }

  key(event) {
    const open = this.open();
    if (open && event.key === "ArrowDown") { event.preventDefault(); this.lit = (this.lit + 1) % this.items.length; this.show(); return; }
    if (open && event.key === "ArrowUp") { event.preventDefault(); this.lit = (this.lit + this.items.length - 1) % this.items.length; this.show(); return; }
    // Enter and Tab take the chosen suggestion only while it is for what is typed now.
    const current = this.input.value.trim() === this.asked;
    if (open && current && (event.key === "Enter" || (event.key === "Tab" && !event.shiftKey))) { event.preventDefault(); this.take(this.lit); return; }
    if (event.key === "Enter") { event.preventDefault(); this.commit(); return; }
    if (event.key === "Escape") {
      if (open) { event.preventDefault(); event.stopPropagation(); this.close(); }
      return;
    }
    if (event.key === "Backspace" && this.input.value === "" && this.chips.length) {
      this.chips.pop();
      this.render();
      changed();
    }
  }
}

const fields = { to: new Recipients("to"), cc: new Recipients("cc"), bcc: new Recipients("bcc") };

function showRow(name, on = true) {
  $(`#row-${name}`).hidden = !on;
  if (on) fields[name].input.focus();
  $(`#show-${name}`)?.toggleAttribute("hidden", on);
}
$("#show-cc").addEventListener("click", () => showRow("cc"));
$("#show-bcc").addEventListener("click", () => showRow("bcc"));

// ---------------------------------------------------------------------------------------------
// The From row, subject, attachments, status
// ---------------------------------------------------------------------------------------------

const fromMenu = $("#from-menu");

function renderFrom() {
  const sender = state.senders[state.from];
  if (!sender) return;
  const named = sender.name || sender.address;
  $("#from-name").textContent = named;
  $("#from-addr").textContent = sender.name ? sender.address : "";
  $("#fromsmall").textContent = state.answer ? sender.small : "";
  const several = state.senders.length > 1;
  $("#from-btn").classList.toggle("single", !several);
  $("#from-btn .chev").style.display = several ? "" : "none";
  // An answer needs no From row: the account stands small in the header.
  $("#row-from").hidden = state.answer && !several;
  $("#fromsmall").hidden = !state.answer || several;
  const signature = $("#signature");
  signature.hidden = !sender.signature;
  $("#signature-text").textContent = sender.signature || "";
  fromMenu.replaceChildren();
  state.senders.forEach((s, index) => {
    const item = document.createElement("button");
    item.type = "button";
    item.className = index === state.from ? "cur" : "";
    item.append(document.createTextNode(s.name || s.address));
    if (s.name) { const address = document.createElement("span"); address.textContent = s.address; item.append(address); }
    item.addEventListener("click", () => { fromMenu.hidden = true; if (index !== state.from) post({ t: "pick_from", index }); });
    fromMenu.append(item);
  });
}
$("#from-btn").addEventListener("click", () => { if (state.senders.length > 1) fromMenu.hidden = !fromMenu.hidden; });

const subjectEl = $("#subject");
subjectEl.addEventListener("input", changed);
subjectEl.addEventListener("focus", () => $("#row-subject").classList.add("focus"));
subjectEl.addEventListener("blur", () => $("#row-subject").classList.remove("focus"));
subjectEl.addEventListener("keydown", (event) => {
  if (event.key === "Enter") { event.preventDefault(); squire.focus(); }
  if (event.key === "Escape") { event.preventDefault(); escapeKey(); }
});

function renderAttachments(items) {
  const box = $("#atts");
  box.replaceChildren();
  items.forEach((item, index) => {
    const chip = document.createElement("span");
    chip.className = "chip";
    const name = document.createElement("span");
    name.className = "name";
    name.textContent = item.name;
    chip.append(name);
    if (item.size) { const size = document.createElement("small"); size.textContent = item.size; chip.append(size); }
    const x = document.createElement("button");
    x.className = "x";
    x.type = "button";
    x.title = T("Remove the attachment");
    x.innerHTML = ICON_X;
    x.addEventListener("click", () => post({ t: "remove_attachment", index }));
    chip.append(x);
    box.append(chip);
  });
}

function renderStatus(event) {
  const words = { sending: T("Sending…"), saving: T("Saving…"), unsaved: T("Unsaved changes"), saved: T("Draft saved"), clean: "" };
  $("#status").textContent = words[event.state] ?? "";
  state.locked = !!event.locked;
  state.sending = event.state === "sending";
  $("#app").classList.toggle("locked", state.locked);
  $("#send").disabled = state.sending;
  document.title = event.title || "";
  asking(event.asking);
}

function renderNotice(event) {
  const notice = $("#notice");
  if (!event.what) { notice.hidden = true; return; }
  $("#notice-what").textContent = event.what;
  $("#notice-why").textContent = event.why || "";
  notice.hidden = false;
}
$("#notice-close").addEventListener("click", () => post({ t: "dismiss_notice" }));

// ----- dialogs -----

function asking(kind) {
  if (state.asking === kind) return;
  state.asking = kind;
  const scrim = $("#scrim");
  if (!kind) { scrim.hidden = true; return; }
  const actions = $("#dialog-actions");
  actions.replaceChildren();
  const button = (text, message, className) => {
    const b = document.createElement("button");
    b.type = "button";
    b.textContent = text;
    if (className) b.className = className;
    b.addEventListener("click", () => post({ t: message }));
    actions.append(b);
    return b;
  };
  if (kind === "close") {
    $("#dialog-title").textContent = T("Save this draft?");
    $("#dialog-text").textContent = T("Your latest changes aren't saved yet.");
    button(T("Discard changes"), "discard_changes", "danger");
    button(T("Keep writing"), "keep_writing");
    button(T("Save draft"), "save_and_close", "primary").focus();
  } else {
    $("#dialog-title").textContent = T("Discard this draft?");
    $("#dialog-text").textContent = T("It moves to the trash.");
    button(T("Cancel"), "keep_writing");
    button(T("Discard draft"), "confirm_discard", "danger").focus();
  }
  scrim.hidden = false;
}

// ---------------------------------------------------------------------------------------------
// What the person does
// ---------------------------------------------------------------------------------------------

let formTimer = null;
function changed() {
  if (!state.ready) return;
  clearTimeout(formTimer);
  formTimer = setTimeout(() => sendForm(false), 200);
}

function sendForm(baseline) {
  clearTimeout(formTimer);
  post({
    t: "form",
    to: fields.to.data(), cc: fields.cc.data(), bcc: fields.bcc.data(),
    subject: subjectEl.value,
    html: editorHtml(),
    baseline,
  });
}

function send() {
  if (!state.ready || state.sending) return;
  sendForm(false);
  post({ t: "send" });
}
$("#send").addEventListener("click", send);
$("#close").addEventListener("click", () => { sendForm(false); post({ t: "close" }); });
$("#attach").addEventListener("click", () => post({ t: "attach" }));
$("#discard-draft").addEventListener("click", () => { sendForm(false); post({ t: "discard_draft" }); });
$("#failed-retry").addEventListener("click", () => post({ t: "retry" }));
$("#failed-close").addEventListener("click", () => post({ t: "close" }));

// Esc closes what is open, from the inside out: the list, a menu, a popover; then the window asks.
function hidePopovers() { linkBar.hidden = true; deselectImage(); fromMenu.hidden = true; }
function escapeKey() {
  if (!linkBar.hidden || state.selected || !fromMenu.hidden) { hidePopovers(); squire.focus(); return; }
  sendForm(false);
  post({ t: "close" });
}

document.addEventListener("keydown", (event) => {
  const mod = MAC ? event.metaKey : event.ctrlKey;
  // The shortcut "send" of the system's preset: Ctrl+Enter; on macOS also Cmd+Enter and Shift+Cmd+D.
  if ((mod && event.key === "Enter") || (MAC && event.metaKey && event.shiftKey && event.key.toLowerCase() === "d")) {
    event.preventDefault();
    send();
  } else if (event.key === "Escape" && !event.defaultPrevented) {
    event.preventDefault();
    escapeKey();
  }
});
document.addEventListener("mousedown", (event) => {
  if (!event.target.closest?.("#from-menu, #from-btn")) fromMenu.hidden = true;
  if (!event.target.closest?.("#linkbar, [data-cmd=link], #imgbar, #editor")) hidePopovers();
});

$("#aa").addEventListener("click", () => {
  const tools = $("#tools");
  const folded = tools.classList.toggle("folded");
  $("#aa").dataset.title = folded ? T("Show formatting") : T("Hide formatting");
  $("#aa").title = $("#aa").dataset.title;
  $("#aa").classList.toggle("on", !folded);
  post({ t: "format_bar", on: !folded });
});

$("#paper-toggle").addEventListener("click", () => post({ t: "toggle_paper" }));
function setPaper(paper) {
  state.paper = paper;
  document.body.dataset.paper = paper;
  const next = paper === "dark" ? T("Switch to light paper") : T("Switch to dark paper");
  $("#paper-toggle").title = next;
  $("#paper-toggle").dataset.title = next;
}

$("#history-toggle").addEventListener("click", () => {
  const folded = editorEl.classList.toggle("fold-history");
  $("#history-toggle").textContent = folded ? T("Show history") : T("Hide history");
});

// ----- drops -----

const dropzone = $("#dropzone");
function dropMessage(x, y) {
  const over = overText(x, y);
  $("#dropzone-text").textContent = over ? T("Drop to embed the picture here") : T("Drop files to attach them");
}
function overText(x, y) {
  const el = document.elementFromPoint(x, y);
  return !!el && (editorEl.contains(el) || el === editorEl);
}
function caretAt(x, y) {
  const position = document.caretPositionFromPoint ? document.caretPositionFromPoint(x, y)
    : (document.caretRangeFromPoint ? document.caretRangeFromPoint(x, y) : null);
  if (!position) return null;
  const range = document.createRange();
  if (position.offsetNode) { range.setStart(position.offsetNode, position.offset); }
  else { range.setStart(position.startContainer, position.startOffset); }
  range.collapse(true);
  return range;
}
// Files that no longer than the page are looked at come through the program (`drag`, `drop`); a drag inside the
// text is the editor's own. Nothing else is accepted by the page: a stray drop never navigates.
for (const kind of ["dragover", "drop"]) {
  document.addEventListener(kind, (event) => {
    if (event.dataTransfer && [...event.dataTransfer.types].includes("Files")) event.preventDefault();
  });
}

function dragEvent(event) {
  if (!event.on) { dropzone.hidden = true; return; }
  dropMessage(event.x, event.y);
  dropzone.hidden = false;
}

function dropEvent(event) {
  dropzone.hidden = true;
  const embed = overText(event.x, event.y);
  const range = embed ? caretAt(event.x, event.y) : null;
  const token = ++state.token;
  if (range) state.inserts[token] = range;
  post({ t: "files", paths: event.paths, embed, token });
}

// ---------------------------------------------------------------------------------------------
// What the program says
// ---------------------------------------------------------------------------------------------

function applyTheme(event) {
  strings = event.strings || {};
  if (event.appearance === "light" || event.appearance === "dark") document.documentElement.dataset.theme = event.appearance;
  const rules = (vars) => Object.entries(vars).map(([name, value]) => `${name}:${value}`).join(";");
  const style = document.createElement("style");
  style.textContent = `:root{${rules(event.palette.light)}}@media (prefers-color-scheme:dark){:root:not([data-theme="light"]){${rules(event.palette.dark)}}}` +
    `:root[data-theme="dark"]{${rules(event.palette.dark)}}:root[data-theme="light"]{${rules(event.palette.light)}}`;
  document.head.append(style);
  $$("[data-t]").forEach((el) => { el.textContent = T(el.dataset.t); });
  $$("[data-title]").forEach((el) => { el.title = T(el.dataset.title); el.setAttribute("aria-label", T(el.dataset.title)); });
  $$("[data-ph]").forEach((el) => { if (el.tagName === "INPUT") el.placeholder = T(el.dataset.ph); else el.dataset.ph = T(el.dataset.ph); });
  $$(".recipients").forEach((row) => $("input", row).setAttribute("aria-label", $("label", row).textContent));
  subjectEl.setAttribute("aria-label", T("Subject"));
}

function applyInit(event) {
  state.answer = event.answer;
  state.forward = event.forward;
  state.history = event.history;
  state.senders = event.senders;
  state.from = event.from;
  state.writableBook = event.writableBook;
  state.sendKeys = event.sendKeys;
  state.images = {};
  event.images.forEach((image) => { state.images[image.cid] = image; });
  $("#heading").textContent = event.heading;
  document.title = event.title;
  for (const name of ["to", "cc", "bcc"]) fields[name].set(event[name]);
  subjectEl.value = event.subject;
  setEditorHtml(event.html);
  editorEl.classList.toggle("fold-history", !!event.foldHistory);
  const toggle = $("#history-toggle");
  toggle.hidden = !event.history;
  toggle.textContent = event.foldHistory ? T("Show history") : T("Hide history");
  setPaper(event.paper);
  renderFrom();
  const cc = event.cc.chips.length > 0, bcc = event.bcc.chips.length > 0;
  $("#row-cc").hidden = !cc;
  $("#row-bcc").hidden = !bcc;
  $("#show-cc").hidden = cc;
  $("#show-bcc").hidden = bcc;
  $("#send").title = T("Send") + (event.sendKeys ? " (" + event.sendKeys + ")" : "");
  $("#loading").hidden = true;
  $("#failed").hidden = true;
  $("#app").hidden = false;
  state.ready = true;
  // The page draws what it was given first, then says so: what it makes of the HTML is no change.
  squire.moveCursorToStart();
  if (event.focus === "to") fields.to.input.focus(); else squire.focus();
  sendForm(true);
  applyOpen(event.open);
  if (event.selfTest) selfTest();
}

// The demo states of `--demo --open <state>` that are the page's own.
function applyOpen(open) {
  if (open === "cc") { showRow("cc"); showRow("bcc"); fields.to.input.blur(); }
  if (open === "format-off") $("#aa").click();
  if (open === "history") $("#history-toggle").click();
  if (open === "menu") fromMenu.hidden = false;
  if (open === "suggest") { fields.to.input.focus(); fields.to.ask(); }
}

// `TORRO_COMPOSE_SELFTEST=1`: checks in the real web view that no other test can make (does Squire run, does the program's
// own protocol deliver a picture, does the policy hold, does a paste come back cleaned). Reported to the program, which
// prints them and ends.
async function selfTest() {
  const results = [];
  const check = (name, ok, detail = "") => results.push({ name, ok: !!ok, detail: String(detail) });
  const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const selectAll = () => { const range = document.createRange(); range.selectNodeContents(editorEl); squire.setSelection(range); };
  try {
    check("Squire is the editor and DOMPurify is there", typeof Squire === "function" && !!window.DOMPurify, navigator.userAgent);
    await wait(400);
    const img = editorEl.querySelector("img");
    if (img) check("a prepared picture loads through the program's own address", img.complete && img.naturalWidth > 0, img.getAttribute("src") + " " + img.naturalWidth);
    try { await fetch("https://example.org/", { mode: "no-cors" }); check("the policy refuses a connection", false, "it connected"); }
    catch (error) { check("the policy refuses a connection", true, String(error)); }
    try { new Function("return 1")(); check("the policy refuses eval", false, "it ran"); }
    catch (error) { check("the policy refuses eval", true, String(error)); }
    squire.setHTML("<div>Alpha</div><div>Beta</div>");
    // Squire notices changes through a MutationObserver: what is changed in the same turn as setHTML counts as part of it.
    await wait(60);
    squire.focus();
    selectAll();
    squire.bold();
    check("bold shows as bold, not as marks", /<b>Alpha<\/b>/.test(squire.getHTML()) && !squire.getHTML().includes("*"), squire.getHTML());
    await wait(60);
    squire.undo();
    await wait(60);
    check("undo takes the bold back", !squire.getHTML().includes("<b>"), squire.getHTML());
    squire.redo();
    await wait(60);
    check("redo brings it back", squire.getHTML().includes("<b>"), squire.getHTML());
    selectAll();
    squire.makeUnorderedList();
    check("a list is a list", squire.getHTML().includes("<ul>") && squire.getHTML().includes("<li>"), squire.getHTML());
    squire.setHTML("<div>site</div>");
    selectAll();
    squire.makeLink("https://example.org/");
    check("a link keeps its address", /<a href="https:\/\/example.org\/">site<\/a>/.test(squire.getHTML()), squire.getHTML());
    squire.setHTML('<div>One</div><div class="torro-quote"><blockquote type="cite"><div>Quoted</div></blockquote></div><div>Two</div>');
    check("the quoted history and its marks survive the editor", editorHtml().includes('class="torro-quote"') && editorHtml().includes('type="cite"'), editorHtml());
    selectAll();
    check("a selection runs across paragraphs", window.getSelection().toString().includes("One") && window.getSelection().toString().includes("Two"), window.getSelection().toString());
    squire.setHTML('<div><img src="/img/a@torro" alt="A" width="320" height="180" data-cid="a@torro" style="x:y" onerror="x()"></div>');
    await wait(60);
    const kept = squire.getHTML();
    check("a picture keeps its id in the editor and loses handlers and styles", kept.includes('data-cid="a@torro"') && !kept.includes("onerror") && !kept.includes("style"), kept);
    // A paste goes to the program and comes back as what mail allows.
    squire.setHTML("<div>Start </div>");
    squire.moveCursorToEnd();
    const data = new DataTransfer();
    data.setData("text/html", '<b>Pasted</b> <a href="https://example.org/p">link</a><script>steal()</script><img src="https://evil.example/x.png">');
    data.setData("text/plain", "Pasted link");
    editorEl.dispatchEvent(new ClipboardEvent("paste", { clipboardData: data, bubbles: true, cancelable: true }));
    await wait(800);
    const pasted = squire.getHTML();
    check("a paste comes back cleaned and formatted", /<(b|strong)>Pasted<\/(b|strong)>/.test(pasted) && pasted.includes("https://example.org/p") && !pasted.includes("script") && !pasted.includes("evil"), pasted);
    // Typed addresses become chips, a wrong one is marked.
    const field = fields.cc;
    $("#row-cc").hidden = false;
    field.set({ chips: [], typed: "" });
    field.input.value = "anna@example.org, nobody";
    field.typed();
    field.commit();
    await wait(600);
    check("typed addresses become chips and a wrong one is marked", field.chips.length === 2 && field.chips.some((chip) => chip.broken), JSON.stringify(field.chips));
  } catch (error) {
    check("the checks ran without a script error", false, error && error.stack ? error.stack : String(error));
  }
  post({ t: "self_test", results });
}

window.torroCompose = {
  receive(event) {
    switch (event.e) {
      case "theme": applyTheme(event); break;
      case "loading": $("#loading").hidden = false; $("#failed").hidden = true; break;
      case "failed":
        $("#loading").hidden = true;
        $("#failed").hidden = false;
        $("#failed-what").textContent = event.what;
        $("#failed-why").textContent = event.why || "";
        break;
      case "init": applyInit(event); break;
      case "status": renderStatus(event); break;
      case "notice": renderNotice(event); break;
      case "attachments": renderAttachments(event.items); break;
      case "from": state.from = event.from; renderFrom(); break;
      case "paper": setPaper(event.paper); break;
      case "chips": event.chips.forEach((chip) => fields[event.field].add(chip)); break;
      case "suggestions": fields[event.field].suggestions(event.seq, event.items); break;
      case "photo":
        $$(`.av[data-contact="${CSS.escape(event.contact)}"]`).forEach((av) => {
          const img = document.createElement("img");
          img.alt = "";
          img.src = event.url;
          av.replaceChildren(img);
        });
        break;
      case "contact_added":
        if (event.error) { Object.values(fields).forEach((field) => { if (field.open()) { field.problem = event.error; field.show(); } }); }
        else { Object.values(fields).forEach((field) => { if (field.open()) field.ask(); }); }
        break;
      case "image": insertImage(event.token, event.image); break;
      case "paste": pasteResult(event); break;
      case "drag": dragEvent(event); break;
      case "drop": dropEvent(event); break;
      case "close": break;
      default: break;
    }
  },
};

post({ t: "ready" });
