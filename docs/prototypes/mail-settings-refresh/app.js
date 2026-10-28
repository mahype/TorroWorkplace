const accounts = [
  { address: 's@g5k.de', name: 'Sven Wagener', initial: 'S', avatar: '', sync: 'heute 22:15', messages: '4.796', offline: '292', server: '424 MB', local: '63,5 MB', onlineMb: 424, actualPercent: 15, plannedPercent: 23, indexedPercent: 82, imap: 'mail.g5k.de · 993 · TLS', smtp: 'mail.g5k.de · 465 · TLS' },
  { address: 'sven@immoticket24.de', name: 'Sven Wagener', initial: 'I', avatar: 'teal', sync: 'heute 21:48', messages: '14.218', offline: '12.084', server: '2,36 GB', local: '1,18 GB', onlineMb: 2360, actualPercent: 50, plannedPercent: 22, indexedPercent: 94, imap: 'imap.immoticket24.de · 993 · TLS', smtp: 'smtp.immoticket24.de · 587 · STARTTLS' },
  { address: 'sven@awesome.ug', name: 'Sven Wagener', initial: 'A', avatar: 'violet', sync: 'gestern 18:03', messages: '2.960', offline: '2.300', server: '810 MB', local: '610 MB', onlineMb: 810, actualPercent: 75, plannedPercent: 0, indexedPercent: 100, imap: 'imap.awesome.ug · 993 · TLS', smtp: 'smtp.awesome.ug · 465 · TLS' },
];

const descriptions = {
  'list-a': 'A · Dichte Liste: Status und drei nützliche Größen bleiben auf einen Blick sichtbar. Klick auf ein Konto öffnet die Einstellungsseite.',
  'list-b': 'B · Frühere Kartenvariante zum Vergleich. Der Balken konnte wie ein Postfachkontingent wirken.',
  'list-c': 'C · Bevorzugte Übersicht: Größenvergleich pro Konto mit lokalem Ist, geplantem Download und online verbleibendem Anteil.',
  'account-settings': 'Kontodetail · Zugangsdaten links; automatisch erkannte Server und getrennte manuelle Verbindungsangaben rechts.',
  'account-storage': 'Speicher & Suche · Drei eindeutige Offline-Strategien, Anhänge und Indexierungsfortschritt pro Konto.',
  'reading': 'Allgemein › Lesen & Schreiben · Lese- und Schreibvorgaben sowie eine formatierbare Signatur.',
  'attachments': 'Allgemein › Anhänge · Standardaktion und Speicherziele nebeneinander; Details öffnen sich bei Bedarf.',
  'categorization': 'Kategorisierung · Automatische Einsortierung und Kategorien in eigenen Tabs. Regeln gelten vor der Zuordnung per Sprachmodell.',
};

let selectedAccount = 0;
let lastList = 'list-c';
let currentScreen = 'list-c';
let currentCategoryTab = new URLSearchParams(location.search).get('tab') === 'catalog' ? 'catalog' : 'automation';
let lastToast;

function plannedPercent(account) {
  if (account.mode === 'all') return 100 - account.actualPercent;
  if (account.mode === 'limit') return Math.max(0, Math.min(100, Math.round((account.limitMb || 500) / account.onlineMb * 100)) - account.actualPercent);
  return account.plannedPercent;
}

function updateMeter(index) {
  const account = accounts[index];
  const planned = plannedPercent(account);
  const remaining = 100 - account.actualPercent - planned;
  const words = `${account.actualPercent} Prozent auf dem Gerät, weitere ${planned} Prozent geplant, ${remaining} Prozent bleiben online`;
  const row = document.querySelector(`#list-c tr[data-account="${index}"]`);
  row.querySelector('.meter-planned').style.width = `${planned}%`;
  row.querySelector('.local-meter').setAttribute('aria-label', words);
  row.querySelector('.list-index-scope').style.width = `${account.actualPercent}%`;
  row.querySelector('.list-index-filled').style.width = `${account.indexedPercent}%`;
  row.querySelector('.list-index-meter').setAttribute('aria-label', `${account.indexedPercent} Prozent der lokalen Inhalte für die Volltextsuche indiziert`);
  if (index !== selectedAccount) return;
  document.getElementById('storage-actual-bar').style.width = `${account.actualPercent}%`;
  document.getElementById('storage-pending-bar').style.width = `${planned}%`;
  document.getElementById('storage-meter').setAttribute('aria-label', words);
  document.getElementById('storage-index-scope').style.width = `${account.actualPercent}%`;
  document.getElementById('storage-index-filled').style.width = `${account.indexedPercent}%`;
  document.getElementById('storage-index-meter').setAttribute('aria-label', `${account.indexedPercent} Prozent der lokalen Inhalte für die Volltextsuche indiziert`);
  document.getElementById('storage-index-label').textContent = `Für Volltextsuche indiziert (${account.indexedPercent} % der lokalen Inhalte)`;
  document.getElementById('storage-target-text').textContent = account.mode === 'limit' && (account.limitMb || 500) / account.onlineMb * 100 < account.actualPercent ? 'Grenze liegt unter dem aktuellen Bestand' : planned ? `Weitere ${planned} % vorgesehen` : 'Keine weiteren Downloads geplant';
}

function toast(message) {
  const box = document.getElementById('toast');
  box.textContent = message;
  box.classList.add('show');
  clearTimeout(lastToast);
  lastToast = setTimeout(() => box.classList.remove('show'), 3500);
}

function serverParts(value) {
  const [host, port, security] = value.split(' · ');
  return { host, port, security };
}

function fillServerFields(account) {
  document.getElementById('incoming-protocol-edit').value = account.incomingProtocol || 'IMAP';
  document.getElementById('incoming-protocol-label').textContent = account.incomingProtocol || 'IMAP';
  document.getElementById('system-folders').hidden = account.incomingProtocol === 'POP3';
  for (const type of ['imap', 'smtp']) {
    const { host, port, security } = serverParts(account[type]);
    document.getElementById(`${type}-server-edit`).value = host;
    document.getElementById(`${type}-port-edit`).value = port;
    document.getElementById(`${type}-security-edit`).value = security;
  }
}

function setAccount(index) {
  selectedAccount = Number(index);
  const account = accounts[selectedAccount];
  for (const [id, value] of [
    ['settings-title', account.address], ['storage-title', account.address],
    ['detail-name', account.name], ['storage-name', account.name],
    ['detail-sync', `${account.sync} synchronisiert`], ['storage-sync', account.sync],
    ['sender-name', account.name],
    ['imap-server', account.imap], ['smtp-server', account.smtp],
    ['server-size', account.server], ['local-size', account.local],
    ['storage-mail-count', `${account.messages} E-Mails bekannt`],
    ['storage-offline-count', `${account.offline} E-Mails offline lesbar`],
    ['storage-device-percent', `${account.actualPercent} % der bekannten Online-Größe`],
    ['backup-account', account.address],
  ]) document.getElementById(id).textContent = value;
  document.getElementById('account-address').value = account.address;
  document.getElementById('account-login').value = account.login || account.address;
  fillServerFields(account);
  for (const id of ['detail-avatar', 'storage-avatar']) {
    const avatar = document.getElementById(id);
    avatar.textContent = account.initial;
    avatar.className = `avatar large ${account.avatar}`;
  }
  const mode = account.mode || 'since';
  document.querySelector(`input[name="policy"][value="${mode}"]`).checked = true;
  document.getElementById('policy-date').hidden = mode !== 'since';
  document.getElementById('policy-limit').hidden = mode !== 'limit';
  document.getElementById('limit-scope').hidden = mode !== 'limit';
  document.getElementById('policy-date-input').value = account.date || '2024-01-01';
  document.getElementById('policy-limit-input').value = account.limitMb || 500;
  document.getElementById('retention').value = account.attachmentRetention || '7';
  updateMeter(selectedAccount);
  document.getElementById('account-password').value = '';
  document.getElementById('save-strip').hidden = true;
}

function showScreen(name) {
  if (name === 'storage-general') name = 'account-storage';
  if (['sorting-rules', 'categories', 'category-editor', 'rule-editor'].includes(name)) name = 'categorization';
  if (!descriptions[name]) name = 'list-c';
  if (name.startsWith('list-')) lastList = name;
  currentScreen = name;
  document.querySelectorAll('.screen').forEach(el => el.classList.toggle('active', el.id === name));
  document.querySelectorAll('.preview-tab').forEach(el => el.classList.toggle('active', el.dataset.screen === name));
  const area = name.startsWith('list-') || name.startsWith('account-') ? 'list-a'
    : ['reading', 'attachments'].includes(name) ? 'reading' : 'categorization';
  document.querySelectorAll('.side-link').forEach(el => el.classList.toggle('active', el.dataset.screen === area));
  document.querySelectorAll('.mobile-link').forEach(el => el.classList.toggle('active', el.dataset.screen === area));
  document.getElementById('preview-note').textContent = descriptions[name];
  const crumb = document.getElementById('breadcrumb');
  if (name.startsWith('list-')) crumb.textContent = 'Einstellungen › Konten';
  else if (name.startsWith('account-')) crumb.textContent = `Einstellungen › Konten › ${accounts[selectedAccount].address} › ${name === 'account-settings' ? 'Einstellungen' : 'Speicher & Suche'}`;
  else if (area === 'reading') crumb.textContent = `Einstellungen › Allgemein › ${{ reading: 'Lesen & Schreiben', attachments: 'Anhänge' }[name]}`;
  else crumb.textContent = 'Einstellungen › Kategorisierung';
  document.getElementById('page-scroll').scrollTop = 0;
  history.replaceState(null, '', `?screen=${name}&account=${selectedAccount}${name === 'categorization' ? `&tab=${currentCategoryTab}` : ''}`);
}

document.querySelectorAll('[data-screen]').forEach(button => button.addEventListener('click', () => showScreen(button.dataset.screen)));
document.querySelectorAll('[data-account]').forEach(row => {
  const open = () => { setAccount(row.dataset.account); showScreen('account-settings'); };
  row.addEventListener('click', open);
  if (row.tagName === 'TR') row.addEventListener('keydown', event => { if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); open(); } });
});
document.querySelectorAll('[data-back]').forEach(button => button.addEventListener('click', () => showScreen(lastList)));
document.querySelectorAll('[data-demo]').forEach(button => button.addEventListener('click', () => toast(button.dataset.demo)));
const categoryGroupsData = [
  { name: 'Wartet auf mich', entries: [
    ['Anfrage', 'Fragen und Bitten, auf die eine Antwort erwartet wird.'],
    ['Auftrag', 'Eine Aufgabe oder Zusage mit einem konkreten nächsten Schritt.'],
    ['Freigabe', 'Eine Bestätigung, Unterschrift oder Entscheidung wird angefordert.'],
    ['Sicherheit', 'Anmeldecodes, Passwortänderungen und Sicherheitswarnungen.'],
    ['Persönlich', 'Nachrichten von Personen ohne offene Aufgabe.'],
    ['Einladung', 'Einladung zu einem Kalendereintrag.', [{ field: 'System', value: 'Kalendereinladung erkannt', fixed: true }]],
  ] },
  { name: 'Belege', entries: [
    ['Rechnung', 'Rechnungen, Zahlungserinnerungen und Zahlungsaufforderungen.', [{ field: 'Absender', value: 'rechnung@beispiel.de' }]],
    ['Zahlungsbeleg', 'Bestätigung einer Zahlung oder ein Kaufbeleg.'],
    ['Bestellung', 'Bestell- und Kaufbestätigungen.'],
    ['Sendung', 'Versandbestätigungen, Sendungsverfolgung und Zustellversuche.'],
    ['Buchung', 'Reisen, Hotels, Tickets und Termine.'],
    ['Abrechnung', 'Kontoauszüge, Gehaltsabrechnungen und Jahresabrechnungen.'],
    ['Schriftstück', 'Verträge, Bescheide und amtliche Schreiben.'],
  ] },
  { name: 'Zur Info', entries: [
    ['Newsletter', 'Regelmäßige redaktionelle Rundschreiben und Abonnements.', [{ field: 'Betreff', value: 'Newsletter' }]],
    ['Werbung', 'Angebote, Rabatte und Produktwerbung.'],
    ['Dienstmeldung', 'Wartungen, geänderte Bedingungen und Statusmeldungen.'],
    ['Netzwerk', 'Benachrichtigungen aus sozialen Netzwerken und Foren.'],
    ['Veranstaltung', 'Hinweise auf Webinare, Konferenzen und andere Veranstaltungen.'],
  ] },
  { name: 'Technik und Müll', entries: [
    ['Unzustellbar', 'Eine E-Mail konnte nicht zugestellt werden.', [{ field: 'System', value: 'Zustellfehler erkannt', fixed: true }]],
    ['Abwesenheit', 'Automatische Antwort oder Abwesenheitsmeldung.', [{ field: 'System', value: 'Automatische Antwort erkannt', fixed: true }]],
    ['Unerwünscht', 'Massen-E-Mails ohne Bezug zur Person.'],
    ['Betrug', 'Täuschungsversuche, falsche Absender und gefährliche Links.'],
    ['Sonstiges', 'Passt zu keiner anderen Kategorie.'],
  ] },
];
let nextMainCategoryId = 1;
let nextCategoryId = 1;
const iconChoices = [
  ['message_circle', 'Nachricht'], ['list_todo', 'Aufgabe'], ['circle_check', 'Bestätigung'],
  ['user_check', 'Zugewiesen'], ['shield_alert', 'Sicherheit'], ['user', 'Person'],
  ['calendar_plus', 'Neuer Termin'], ['text_quote', 'Schriftstück'], ['paperclip', 'Anhang'],
  ['square_check', 'Markiertes Kästchen'], ['send', 'Senden'], ['calendar_days', 'Kalender'],
  ['chart_column', 'Statistik'], ['align_left', 'Text'], ['mail', 'Brief'], ['flag', 'Fahne'],
  ['bell', 'Benachrichtigung'], ['users', 'Personen'], ['video', 'Video'],
  ['mail_open', 'Geöffnete E-Mail'], ['reply', 'Antwort'], ['trash2', 'Papierkorb'],
  ['eye', 'Auge'], ['ellipsis', 'Weitere'], ['tag', 'Etikett'],
];
const colorChoices = [
  ['blue', 'Blau'], ['amber', 'Orange'], ['teal', 'Türkis'],
  ['violet', 'Violett'], ['green', 'Grün'], ['red', 'Rot'],
];
const defaultSubIcons = {
  Anfrage: 'message_circle', Auftrag: 'list_todo', Freigabe: 'circle_check', Sicherheit: 'shield_alert',
  Persönlich: 'user', Einladung: 'calendar_plus', Rechnung: 'text_quote', Zahlungsbeleg: 'paperclip',
  Bestellung: 'square_check', Sendung: 'send', Buchung: 'calendar_days', Abrechnung: 'chart_column',
  Schriftstück: 'align_left', Newsletter: 'mail', Werbung: 'flag', Dienstmeldung: 'bell',
  Netzwerk: 'users', Veranstaltung: 'video', Unzustellbar: 'mail_open', Abwesenheit: 'reply',
  Unerwünscht: 'trash2', Betrug: 'eye', Sonstiges: 'ellipsis',
};
const mainCategories = categoryGroupsData.map((group, index) => ({
  id: `main-${nextMainCategoryId++}`, name: group.name,
  icon: ['list_todo', 'text_quote', 'mail', 'shield_alert'][index],
  color: ['blue', 'amber', 'teal', 'violet'][index],
  entries: group.entries.map(([name, description, rules]) => ({
    id: `category-${nextCategoryId++}`, name, description, icon: defaultSubIcons[name] || 'mail', rules: rules || [],
  })),
}));
const categoryHost = document.getElementById('category-list');
const escapeHtml = value => String(value).replace(/[&<>"']/g, character => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[character]);
const iconMarkup = icon => `<svg aria-hidden="true"><use href="#i-${iconChoices.some(([key]) => key === icon) ? icon : 'tag'}"/></svg>`;
function findMainCategory(id) { return mainCategories.find(main => main.id === id); }
function findCategory(id) { return mainCategories.flatMap(main => main.entries).find(entry => entry.id === id); }
function renderCategories() {
  categoryHost.innerHTML = mainCategories.map(main => `<section class="main-category" data-main-id="${main.id}" data-color="${main.color}">
      <header class="main-category-head"><span class="main-category-glyph">${iconMarkup(main.icon)}</span><div class="main-category-title"><h3>${escapeHtml(main.name)}</h3></div><div class="main-category-actions"><button class="button category-small-action" type="button" data-edit-main>Bearbeiten</button><button class="button category-small-action" type="button" data-add-sub>+ Unterkategorie</button></div></header>
      <div class="category-entry-list">${main.entries.map(entry => `<article class="category-entry" data-category-id="${entry.id}" id="${entry.id}">
        <header class="category-entry-head"><span class="subcategory-icon">${iconMarkup(entry.icon)}</span><h4 class="category-entry-name">${escapeHtml(entry.name)}</h4><button class="subcategory-edit" type="button" data-edit-sub aria-label="${escapeHtml(entry.name)} bearbeiten" title="Unterkategorie bearbeiten"><svg aria-hidden="true"><use href="#i-pencil"/></svg></button></header>
        <div class="category-entry-body"><div class="category-description-editor"><label class="category-field-label" for="description-${entry.id}">Beschreibung</label><textarea id="description-${entry.id}" data-description rows="2" placeholder="Welche E-Mails gehören hierher?">${escapeHtml(entry.description)}</textarea></div></div>
      </article>`).join('') || '<p class="main-category-empty">Noch keine Unterkategorien.</p>'}</div>
    </section>`).join('');
}
const ruleFieldChoices = [
  { key: 'Absender', label: 'Absenderadresse', operators: ['ist', 'ist nicht', 'enthält', 'enthält nicht'] },
  { key: 'Domain', label: 'Absenderdomain', operators: ['ist', 'ist nicht'] },
  { key: 'Antwortadresse', label: 'Antwortadresse', operators: ['ist', 'ist nicht', 'enthält'] },
  { key: 'Empfänger', label: 'Empfänger (An/Cc)', operators: ['ist', 'ist nicht', 'enthält'] },
  { key: 'Betreff', label: 'Betreff', operators: ['enthält', 'enthält nicht', 'beginnt mit', 'endet mit'] },
  { key: 'Text', label: 'Nachrichtentext', operators: ['enthält', 'enthält nicht'] },
  { key: 'Anhang', label: 'Anhang', operators: ['vorhanden', 'nicht vorhanden'], noValue: true },
  { key: 'Anhangname', label: 'Anhangname oder Dateityp', operators: ['ist', 'enthält', 'endet mit'] },
  { key: 'Größe', label: 'Nachrichtengröße', operators: ['größer als', 'kleiner als'], number: true },
  { key: 'List-ID', label: 'Mailingliste (List-ID)', operators: ['ist', 'enthält'] },
  { key: 'Konto', label: 'E-Mail-Konto', operators: ['ist', 'ist nicht'] },
];
categoryHost.addEventListener('input', event => {
  if (!event.target.matches('[data-description]')) return;
  const entry = event.target.closest('.category-entry');
  const category = findCategory(entry.dataset.categoryId);
  category.description = event.target.value;
});
categoryHost.addEventListener('click', event => {
  const button = event.target.closest('button');
  if (!button) return;
  const mainElement = button.closest('.main-category');
  if (button.hasAttribute('data-edit-main')) { openMainCategoryDialog(mainElement.dataset.mainId); return; }
  if (button.hasAttribute('data-add-sub')) { openSubcategoryDialog(mainElement.dataset.mainId); return; }
  const entry = button.closest('.category-entry');
  if (entry && button.hasAttribute('data-edit-sub')) openSubcategoryDialog(mainElement.dataset.mainId, entry.dataset.categoryId);
});
const mainCategoryDialog = document.getElementById('main-category-dialog');
const subcategoryDialog = document.getElementById('subcategory-dialog');
for (const id of ['main-category-icon', 'subcategory-icon']) {
  document.getElementById(id).innerHTML = iconChoices.map(([key, label]) => `<label class="category-icon-option" title="${escapeHtml(label)}"><input type="radio" name="${id}" value="${key}" aria-label="${escapeHtml(label)}">${iconMarkup(key)}</label>`).join('');
}
document.getElementById('main-category-color').innerHTML = colorChoices.map(([key, label]) => `<option value="${key}">${label}</option>`).join('');
function chosenIcon(id) { return document.querySelector(`#${id} input:checked`)?.value || 'tag'; }
function selectIcon(id, icon) {
  const input = document.querySelector(`#${id} input[value="${icon}"]`) || document.querySelector(`#${id} input[value="tag"]`);
  input.checked = true;
}
let editingMainId = null;
let editingSubId = null;
function updateMainCategoryPreview() {
  const preview = document.getElementById('main-category-preview');
  const color = document.getElementById('main-category-color').value;
  preview.dataset.color = color;
  document.getElementById('main-category-icon').dataset.color = color;
  preview.querySelector('.main-category-glyph').innerHTML = iconMarkup(chosenIcon('main-category-icon'));
  document.getElementById('main-category-preview-name').textContent = document.getElementById('main-category-name').value.trim() || 'Neue Hauptkategorie';
}
function updateSubcategoryPickerColor() {
  const main = findMainCategory(document.getElementById('subcategory-parent').value);
  document.getElementById('subcategory-icon').dataset.color = main?.color || 'blue';
}
function openMainCategoryDialog(id = null) {
  editingMainId = id;
  const main = id ? findMainCategory(id) : null;
  document.getElementById('main-category-title').textContent = main ? 'Hauptkategorie bearbeiten' : 'Neue Hauptkategorie';
  document.getElementById('main-category-save').textContent = main ? 'Speichern' : 'Hauptkategorie anlegen';
  document.getElementById('main-category-name').value = main?.name || '';
  selectIcon('main-category-icon', main?.icon || 'tag');
  document.getElementById('main-category-color').value = main?.color || 'blue';
  updateMainCategoryPreview();
  mainCategoryDialog.showModal();
  document.getElementById('main-category-name').focus();
}
function openSubcategoryDialog(mainId, subId = null) {
  editingSubId = subId;
  const sub = subId ? findCategory(subId) : null;
  document.getElementById('subcategory-title').textContent = sub ? 'Unterkategorie bearbeiten' : 'Neue Unterkategorie';
  document.getElementById('subcategory-save').textContent = sub ? 'Speichern' : 'Unterkategorie anlegen';
  document.getElementById('subcategory-name').value = sub?.name || '';
  selectIcon('subcategory-icon', sub?.icon || 'tag');
  const parentSelect = document.getElementById('subcategory-parent');
  parentSelect.innerHTML = mainCategories.map(main => `<option value="${main.id}">${escapeHtml(main.name)}</option>`).join('');
  parentSelect.value = mainId;
  updateSubcategoryPickerColor();
  subcategoryDialog.showModal();
  document.getElementById('subcategory-name').focus();
}
document.getElementById('new-main-category').addEventListener('click', () => openMainCategoryDialog());
for (const id of ['main-category-name', 'main-category-color']) document.getElementById(id).addEventListener('input', updateMainCategoryPreview);
document.getElementById('main-category-icon').addEventListener('change', updateMainCategoryPreview);
document.getElementById('subcategory-parent').addEventListener('change', updateSubcategoryPickerColor);
for (const id of ['main-category-close', 'main-category-cancel']) document.getElementById(id).addEventListener('click', () => mainCategoryDialog.close());
for (const id of ['subcategory-close', 'subcategory-cancel']) document.getElementById(id).addEventListener('click', () => subcategoryDialog.close());
document.getElementById('main-category-save').addEventListener('click', () => {
  const nameInput = document.getElementById('main-category-name');
  const name = nameInput.value.trim();
  if (!name || mainCategories.some(main => main.id !== editingMainId && main.name.toLocaleLowerCase('de') === name.toLocaleLowerCase('de'))) { nameInput.focus(); toast('Bitte einen eindeutigen Namen eingeben.'); return; }
  const icon = chosenIcon('main-category-icon');
  const color = document.getElementById('main-category-color').value;
  const main = editingMainId ? findMainCategory(editingMainId) : { id: `main-${nextMainCategoryId++}`, entries: [] };
  Object.assign(main, { name, icon, color });
  if (!editingMainId) mainCategories.push(main);
  mainCategoryDialog.close();
  renderCategories();
  categoryHost.querySelector(`[data-main-id="${main.id}"]`).scrollIntoView({ block: 'nearest' });
});
document.getElementById('subcategory-save').addEventListener('click', () => {
  const nameInput = document.getElementById('subcategory-name');
  const name = nameInput.value.trim();
  if (!name || mainCategories.some(main => main.entries.some(entry => entry.id !== editingSubId && entry.name.toLocaleLowerCase('de') === name.toLocaleLowerCase('de')))) { nameInput.focus(); toast('Bitte einen eindeutigen Namen eingeben.'); return; }
  const targetMain = findMainCategory(document.getElementById('subcategory-parent').value);
  const icon = chosenIcon('subcategory-icon');
  let entry = editingSubId ? findCategory(editingSubId) : null;
  if (entry) {
    const sourceMain = mainCategories.find(main => main.entries.includes(entry));
    if (sourceMain !== targetMain) {
      sourceMain.entries.splice(sourceMain.entries.indexOf(entry), 1);
      targetMain.entries.push(entry);
    }
    Object.assign(entry, { name, icon });
  } else {
    entry = { id: `category-${nextCategoryId++}`, name, description: '', icon, rules: [] };
    targetMain.entries.push(entry);
  }
  subcategoryDialog.close();
  renderCategories();
  const newEntry = categoryHost.querySelector(`[data-category-id="${entry.id}"]`);
  newEntry.scrollIntoView({ block: 'nearest' });
  newEntry.querySelector('[data-description]').focus({ preventScroll: true });
});
renderCategories();

const targetDialog = document.getElementById('target-dialog');
document.querySelectorAll('[data-open-target]').forEach(button => button.addEventListener('click', () => targetDialog.showModal()));
document.querySelectorAll('[data-close-target]').forEach(button => button.addEventListener('click', () => targetDialog.close()));
const theme = document.getElementById('theme-toggle');
theme.addEventListener('click', () => {
  const light = document.documentElement.dataset.theme !== 'light';
  document.documentElement.dataset.theme = light ? 'light' : 'dark';
  theme.textContent = light ? 'Dunkel' : 'Hell';
});

const sender = document.getElementById('sender-name');
const email = document.getElementById('account-address');
const login = document.getElementById('account-login');
const password = document.getElementById('account-password');
const saveStrip = document.getElementById('save-strip');
const dirty = () => { saveStrip.hidden = false; };
for (const field of [sender, email, login, password, document.getElementById('incoming-protocol-edit'), ...['imap', 'smtp'].flatMap(type => [document.getElementById(`${type}-server-edit`), document.getElementById(`${type}-port-edit`), document.getElementById(`${type}-security-edit`)])]) field.addEventListener('input', dirty);
const incomingProtocol = document.getElementById('incoming-protocol-edit');
const incomingPort = document.getElementById('imap-port-edit');
const incomingSecurity = document.getElementById('imap-security-edit');
incomingProtocol.addEventListener('change', () => {
  const defaults = { IMAP: { TLS: '993', STARTTLS: '143' }, POP3: { TLS: '995', STARTTLS: '110' } };
  const previous = incomingProtocol.value === 'POP3' ? 'IMAP' : 'POP3';
  if (incomingPort.value === defaults[previous][incomingSecurity.value]) incomingPort.value = defaults[incomingProtocol.value][incomingSecurity.value];
  document.getElementById('system-folders').hidden = incomingProtocol.value === 'POP3';
  dirty();
});
for (const type of ['imap', 'smtp']) {
  const security = document.getElementById(`${type}-security-edit`);
  const port = document.getElementById(`${type}-port-edit`);
  security.addEventListener('change', () => {
    const defaults = type === 'smtp' ? { TLS: '465', STARTTLS: '587' } : incomingProtocol.value === 'POP3' ? { TLS: '995', STARTTLS: '110' } : { TLS: '993', STARTTLS: '143' };
    const oldSecurity = security.value === 'TLS' ? 'STARTTLS' : 'TLS';
    if (port.value === defaults[oldSecurity]) port.value = defaults[security.value];
    dirty();
  });
}
document.getElementById('test-credentials').addEventListener('click', () => toast('Vorschau: Posteingang und SMTP-Zugang würden mit diesen Angaben geprüft.'));
document.getElementById('discard-settings').addEventListener('click', () => {
  const account = accounts[selectedAccount];
  sender.value = account.name;
  email.value = account.address;
  login.value = account.login || account.address;
  password.value = '';
  fillServerFields(account);
  saveStrip.hidden = true;
});
document.getElementById('save-settings').addEventListener('click', () => {
  const serverInputs = ['imap', 'smtp'].flatMap(type => [document.getElementById(`${type}-server-edit`), document.getElementById(`${type}-port-edit`)]);
  for (const input of serverInputs) if (!input.reportValidity()) return;
  const account = accounts[selectedAccount];
  account.login = login.value.trim();
  account.incomingProtocol = incomingProtocol.value;
  document.getElementById('incoming-protocol-label').textContent = account.incomingProtocol;
  for (const type of ['imap', 'smtp']) {
    account[type] = [document.getElementById(`${type}-server-edit`).value.trim(), document.getElementById(`${type}-port-edit`).value, document.getElementById(`${type}-security-edit`).value].join(' · ');
    document.getElementById(`${type}-server`).textContent = account[type];
  }
  saveStrip.hidden = true;
  toast('Vorschau: Einstellungen wären jetzt gespeichert.');
});

document.querySelectorAll('input[name=policy]').forEach(input => input.addEventListener('change', () => {
  accounts[selectedAccount].mode = input.value;
  document.getElementById('policy-date').hidden = input.value !== 'since';
  document.getElementById('policy-limit').hidden = input.value !== 'limit';
  document.getElementById('limit-scope').hidden = input.value !== 'limit';
  updateMeter(selectedAccount);
}));
document.getElementById('policy-date-input').addEventListener('change', event => { accounts[selectedAccount].date = event.target.value; });
document.getElementById('policy-limit-input').addEventListener('input', event => {
  accounts[selectedAccount].limitMb = Math.max(1, Number(event.target.value) || 500);
  updateMeter(selectedAccount);
});
document.getElementById('retention').addEventListener('change', event => { accounts[selectedAccount].attachmentRetention = event.target.value; });

const signatureEditor = document.getElementById('signature-editor');
const signatureDialog = document.getElementById('signature-link-dialog');
let signatureRange;
function rememberSignatureSelection() {
  const selection = window.getSelection();
  if (selection.rangeCount && signatureEditor.contains(selection.anchorNode)) signatureRange = selection.getRangeAt(0).cloneRange();
}
function restoreSignatureSelection() {
  if (!signatureRange) return;
  const selection = window.getSelection();
  selection.removeAllRanges();
  selection.addRange(signatureRange);
}
signatureEditor.addEventListener('keyup', rememberSignatureSelection);
signatureEditor.addEventListener('mouseup', rememberSignatureSelection);
document.querySelectorAll('[data-signature-format]').forEach(button => {
  button.addEventListener('mousedown', event => event.preventDefault());
  button.addEventListener('click', () => {
    signatureEditor.focus();
    restoreSignatureSelection();
    document.execCommand(button.dataset.signatureFormat, false);
    rememberSignatureSelection();
  });
});
const signatureLinkButton = document.getElementById('signature-add-link');
signatureLinkButton.addEventListener('mousedown', event => event.preventDefault());
signatureLinkButton.addEventListener('click', () => {
  rememberSignatureSelection();
  document.getElementById('signature-link-url').value = '';
  signatureDialog.showModal();
  document.getElementById('signature-link-url').focus();
});
for (const id of ['signature-link-close', 'signature-link-cancel']) document.getElementById(id).addEventListener('click', () => signatureDialog.close());
document.getElementById('signature-link-apply').addEventListener('click', () => {
  const raw = document.getElementById('signature-link-url').value.trim();
  let url;
  try { url = new URL(raw); } catch { toast('Bitte eine gültige Webadresse eingeben.'); return; }
  if (!['http:', 'https:', 'mailto:'].includes(url.protocol)) { toast('Bitte eine Web- oder E-Mail-Adresse eingeben.'); return; }
  signatureDialog.close();
  signatureEditor.focus();
  restoreSignatureSelection();
  const selection = window.getSelection();
  if (selection.rangeCount && !selection.getRangeAt(0).collapsed) {
    document.execCommand('createLink', false, url.href);
  } else {
    const link = document.createElement('a');
    link.href = url.href;
    link.textContent = url.href;
    const range = selection.rangeCount ? selection.getRangeAt(0) : document.createRange();
    if (!selection.rangeCount) range.selectNodeContents(signatureEditor);
    range.collapse(false);
    range.insertNode(link);
    range.setStartAfter(link);
    range.collapse(true);
    selection.removeAllRanges();
    selection.addRange(range);
  }
  rememberSignatureSelection();
});

const dialog = document.getElementById('backup-dialog');
const dateField = document.getElementById('backup-date-field');
const dateInput = document.getElementById('backup-date-input');
const popover = document.getElementById('calendar-popover');
const calendarButton = document.getElementById('calendar-button');
dialog.append(popover);
let calendarMonth = new Date(2024, 0, 1);

function parseDate(value) {
  const match = /^(\d{2})\.(\d{2})\.(\d{4})$/.exec(value.trim());
  if (!match) return null;
  const day = Number(match[1]), month = Number(match[2]) - 1, year = Number(match[3]);
  const date = new Date(year, month, day);
  return date.getFullYear() === year && date.getMonth() === month && date.getDate() === day ? date : null;
}
function formatDate(date) { return `${String(date.getDate()).padStart(2, '0')}.${String(date.getMonth() + 1).padStart(2, '0')}.${date.getFullYear()}`; }
function drawCalendar() {
  document.getElementById('calendar-month').textContent = new Intl.DateTimeFormat('de-DE', { month: 'long', year: 'numeric' }).format(calendarMonth);
  const days = document.getElementById('calendar-days');
  days.replaceChildren();
  const year = calendarMonth.getFullYear(), month = calendarMonth.getMonth();
  const first = new Date(year, month, 1);
  const offset = (first.getDay() + 6) % 7;
  const chosen = parseDate(dateInput.value);
  for (let i = 0; i < 42; i++) {
    const day = new Date(year, month, 1 - offset + i);
    const button = document.createElement('button');
    button.type = 'button';
    button.textContent = day.getDate();
    button.setAttribute('aria-label', new Intl.DateTimeFormat('de-DE', { dateStyle: 'full' }).format(day));
    if (day.getMonth() !== month) button.classList.add('other');
    if (chosen && day.getTime() === chosen.getTime()) button.classList.add('selected');
    button.addEventListener('click', () => { dateInput.value = formatDate(day); dateInput.setCustomValidity(''); calendarMonth = new Date(day.getFullYear(), day.getMonth(), 1); closeCalendar(); });
    days.append(button);
  }
}
function closeCalendar() { popover.hidden = true; calendarButton.setAttribute('aria-expanded', 'false'); }
calendarButton.addEventListener('click', () => {
  const shouldOpen = popover.hidden;
  if (shouldOpen) {
    const chosen = parseDate(dateInput.value);
    if (chosen) calendarMonth = new Date(chosen.getFullYear(), chosen.getMonth(), 1);
    drawCalendar();
    const rect = calendarButton.getBoundingClientRect();
    const dialogRect = dialog.getBoundingClientRect();
    const sideRoom = window.innerWidth - dialogRect.right;
    const left = sideRoom >= 318 ? dialogRect.right + 8
      : dialogRect.left >= 318 ? dialogRect.left - 306
      : Math.min(window.innerWidth - 310, Math.max(12, rect.right - 298));
    const top = sideRoom >= 318 || dialogRect.left >= 318
      ? Math.min(window.innerHeight - 292, rect.top)
      : Math.max(12, rect.top - 292);
    popover.style.left = `${left}px`;
    popover.style.top = `${top}px`;
  }
  popover.hidden = !shouldOpen;
  calendarButton.setAttribute('aria-expanded', String(shouldOpen));
});
document.getElementById('month-prev').addEventListener('click', () => { calendarMonth.setMonth(calendarMonth.getMonth() - 1); drawCalendar(); });
document.getElementById('month-next').addEventListener('click', () => { calendarMonth.setMonth(calendarMonth.getMonth() + 1); drawCalendar(); });
dateInput.addEventListener('change', () => { dateInput.setCustomValidity(parseDate(dateInput.value) ? '' : 'Bitte ein gültiges Datum als TT.MM.JJJJ eingeben.'); });
document.querySelectorAll('input[name=backup-period]').forEach(input => input.addEventListener('change', () => { dateField.hidden = input.value !== 'since'; if (input.value !== 'since') dateInput.setCustomValidity(''); closeCalendar(); }));
document.querySelectorAll('[data-open-backup]').forEach(button => button.addEventListener('click', () => { document.getElementById('backup-account').textContent = accounts[selectedAccount].address; dateInput.setCustomValidity(''); dialog.showModal(); }));
document.getElementById('close-backup').addEventListener('click', () => dialog.close());
document.getElementById('cancel-backup').addEventListener('click', () => dialog.close());
dialog.addEventListener('close', closeCalendar);
document.getElementById('backup-form').addEventListener('submit', event => {
  event.preventDefault();
  if (document.querySelector('input[name=backup-period]:checked').value === 'since' && !parseDate(dateInput.value)) { dateInput.setCustomValidity('Bitte ein gültiges Datum als TT.MM.JJJJ eingeben.'); dateInput.reportValidity(); return; }
  dateInput.setCustomValidity(''); dialog.close(); toast('Vorschau: Das Backup würde jetzt gestartet.');
});

const params = new URLSearchParams(location.search);
const startingAccount = Number(params.get('account'));
accounts.forEach((_, index) => updateMeter(index));
setAccount(Number.isInteger(startingAccount) && startingAccount >= 0 && startingAccount < accounts.length ? startingAccount : 0);
showScreen(params.get('screen') || 'list-c');
