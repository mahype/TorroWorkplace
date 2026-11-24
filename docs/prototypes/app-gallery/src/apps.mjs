// Inventar der Apps für die Galerie-Studie (Issue #970). Quelle der Wahrheit für die echten Apps ist
// `torro_core::AppId::ALL` (crates/torro-core/src/app.rs) und der Katalog (`crates/torroworkplace/src/ui/apps.rs`,
// Texte in `words()`); die geplanten Apps stammen aus docs/app-catalog.md, docs/notizen.md, docs/fotos.md und
// den Issues #348/#367 (Pinboard). Alle Nutzertexte der Mockups sind englisch, wie die Software selbst.
//
// status:  'catalog'  steht heute im App-Katalog (Übersicht der Steuerzentrale)
//          'host'     Hauptanwendung, kein Katalogeintrag (nur „Update…“ als Frage-Karte)
//          'planned'  Konzept oder Entwurf, noch kein Code im Katalog
//          'parked'   zurückgestellt (Pinboard)
// Die Farben je App sind ein VORSCHLAG (Entscheidungspunkt 1 im README): die drei Eingangsfenster behalten ihre
// Farben aus `shell::app_hue` (blau, orange, rosa), Aufgaben grün und Kalender violett sind Slots der
// Standard-Kalenderpalette, der Rest ist neu und im README auf Kontrast geprüft. Befund der Prüfung: das Hell-Grün der
// Kalenderpalette (#1F9D68) erreicht auf der eigenen Tönung nur 2,88:1; für Aufgaben gilt im Vorschlag #1A9262 (3,25:1).

export const FAMILIES = [
  { id: 'inbox', label: 'Inbox' },
  { id: 'plan', label: 'Plan' },
  { id: 'people', label: 'People' },
  { id: 'ai', label: 'AI' },
  { id: 'core', label: 'Core' },
  { id: 'soon', label: 'Planned' },
];

export const APPS = [
  {
    id: 'mail', name: 'Torro Mail', short: 'Mail', cls: 'torro-inbox-mail', bin: 'torroinbox --view mail',
    status: 'catalog', family: 'inbox', icon: 'mail', hue: { dark: '#5B8CFF', light: '#3B6EF0' },
    q: 'What came in by mail?',
    s: 'Mail for the tiled desktop: IMAP accounts, categories, filters and a backup of every mailbox.',
    ver: '0.2.0', size: '24.6 MB', cardState: 'update', from: '0.1.0',
    welcome: {
      headline: 'Mail that keeps up with your tiles', sub: 'Connect an account and the inbox sorts itself into categories you can read at a glance.',
      actions: [
        { icon: 'plus', t: 'Add a mail account', d: 'IMAP and SMTP, with a certificate check' },
        { icon: 'square-pen', t: 'Write an email', d: 'Opens in a window of its own', k: '⌘N' },
        { icon: 'archive', t: 'Back up a mailbox', d: 'A full copy on your own disk' },
      ],
      peek: { title: 'Today', rows: [['Anna Berg', 'Review of the API concept', '09:41'], ['Torro Workplace', 'Your weekly summary', '08:12'], ['Nextcloud', 'Shared folder “Offers”', 'Mon']] },
      tips: ['Categories colour what a mail is', 'A floating bar holds every action'],
    },
  },
  {
    id: 'calendar', name: 'Torro Calendar', short: 'Calendar', cls: 'torro-calendar', bin: 'torrocalendar',
    status: 'catalog', family: 'plan', icon: 'calendar', hue: { dark: '#A78BFA', light: '#7C5CE6' },
    q: "What's coming up?",
    s: 'Your days and weeks from Nextcloud and other CalDAV calendars, with reminders on every display.',
    ver: '0.2.0', size: '24.6 MB', cardState: 'update', from: '0.1.0',
    welcome: {
      headline: 'Your week, on every display', sub: 'Calendars from Nextcloud and any CalDAV server, with reminders that find you on the right screen.',
      actions: [
        { icon: 'cloud', t: 'Connect a calendar', d: 'Nextcloud, iCloud, Fastmail, Radicale' },
        { icon: 'calendar-plus', t: 'New event', d: 'Quick entry, with room for details', k: '⌘N' },
        { icon: 'upload', t: 'Import an .ics file', d: 'Bring events from anywhere' },
      ],
      peek: { title: 'Up next', rows: [['Stand-up', '09:30 – 09:45', 'in 12 min'], ['Design review', '11:00 – 12:00', 'Room 2'], ['Lunch with Jonas', '12:30 – 13:30', '2 h free']] },
      tips: ['Free time shows as “2 h free”', 'Reminders ring on every display'],
    },
  },
  {
    id: 'tasks', name: 'Torro Tasks', short: 'Tasks', cls: 'torro-tasks', bin: 'torrotasks',
    status: 'catalog', family: 'plan', icon: 'list-todo', hue: { dark: '#3FB984', light: '#1A9262' },
    q: 'What do I need to do?',
    s: 'Lists from Nextcloud Tasks and GitHub issues in one place, sorted by what is due.',
    ver: '0.2.0', size: '24.6 MB', cardState: 'installed',
    welcome: {
      headline: 'One list for everything due', sub: 'Nextcloud Tasks and GitHub issues side by side, with focus marks for what matters today.',
      actions: [
        { icon: 'cloud', t: 'Connect Nextcloud Tasks', d: 'Your lists sync both ways' },
        { icon: 'github', t: 'Add GitHub issues', d: 'Assigned to you, as tasks' },
        { icon: 'circle-plus', t: 'New task', d: 'Type, press Enter, done', k: '⌘N' },
      ],
      peek: { title: 'Today', rows: [['Review API concept', 'Focus · High', 'Today'], ['Reply to Anna', '2/5 · Next: draft', 'Today'], ['Renew certificate', 'torro/workplace#412', 'Tomorrow']] },
      tips: ['Colour is priority, shape is focus', 'Drag a task to another day'],
    },
  },
  {
    id: 'hub', name: 'Torro Notification Hub', short: 'Notification Hub', cls: 'torro-inbox', bin: 'torroinbox',
    status: 'catalog', family: 'inbox', icon: 'inbox', hue: { dark: '#E8A33D', light: '#B06F12' },
    q: 'What wants my attention?',
    s: 'Approvals, waiting agents and account problems, all in one list you can clear.',
    ver: '0.2.0', size: '24.6 MB', cardState: 'managed', from: '0.1.0',
    welcome: {
      headline: 'Decide in one place', sub: 'Approvals, waiting agents and account problems arrive here, so you can clear them in a row.',
      actions: [
        { icon: 'bot', t: 'Connect an agent', d: 'Claude Code and others report here' },
        { icon: 'shield', t: 'Review approvals', d: 'Nothing happens without you' },
        { icon: 'bell-off', t: 'Mute a noisy source', d: 'For an hour, a day or for good' },
      ],
      peek: { title: 'Needs you', rows: [['Claude Code is waiting', 'Allow “git push”?', '1 min'], ['Account problem', 'anna@work.example.net', '5 min'], ['Approval', 'API key “Raycast”', '1 h']] },
      tips: ['Cards keep their buttons visible', 'Undo for 5 seconds after every change'],
    },
  },
  {
    id: 'feed', name: 'Torro App Feed', short: 'App Feed', cls: 'torro-inbox-notifications', bin: 'torroinbox --view notifications',
    status: 'catalog', family: 'inbox', icon: 'bell', hue: { dark: '#D96CA8', light: '#C2508F' },
    q: 'What did my apps report?',
    s: 'Notifications from GitHub, Nextcloud, Talk, ntfy and your own scripts.',
    ver: '0.2.0', size: '24.6 MB', cardState: 'installed',
    welcome: {
      headline: 'Everything your apps report', sub: 'One calm feed for GitHub, Nextcloud, Talk, ntfy and your own scripts.',
      actions: [
        { icon: 'github', t: 'Connect GitHub', d: 'Reviews, checks and mentions' },
        { icon: 'radio-tower', t: 'Subscribe to ntfy', d: 'Alerts from your servers' },
        { icon: 'app-window', t: 'Send from a script', d: 'One command, one card' },
      ],
      peek: { title: 'Earlier today', rows: [['torro/workplace#970', 'Review requested', '10:02'], ['Talk · Design team', '3 new messages', '09:48'], ['ntfy · backup', 'Urgent: disk 92 %', '07:30']] },
      tips: ['Colour only in the source tile', 'Urgent alerts say “Urgent”'],
    },
  },
  {
    id: 'assistant', name: 'Torro Assistant', short: 'Assistant', cls: 'torro-assistant', bin: 'torroassistant',
    status: 'catalog', family: 'ai', icon: 'sparkles', hue: { dark: '#3CC4D6', light: '#0E8FA3' },
    q: 'Can you do that for me?',
    s: 'Chat with a local or cloud model that may use your mail, calendar and tasks as you allow.',
    ver: '0.2.0', size: '24.6 MB', cardState: 'available',
    welcome: {
      headline: 'Ask, and let it do the work', sub: 'A local or cloud model that uses your mail, calendar and tasks only as far as you allow.',
      actions: [
        { icon: 'sparkles', t: 'Choose a model', d: 'Ollama, OpenRouter or a connected agent' },
        { icon: 'shield', t: 'Set what it may use', d: 'Rights per area, changeable any time' },
        { icon: 'mic', t: 'Talk instead of typing', d: 'Push to talk, with a wake word later' },
      ],
      peek: { title: 'Try asking', rows: [['“What is on my calendar tomorrow?”', 'Calendar', ''], ['“Draft a reply to Anna”', 'Mail', ''], ['“Move my 3 pm to Thursday”', 'Calendar', '']] },
      tips: ['Every tool call shows as a card', 'You approve what changes data'],
    },
  },
  {
    id: 'chat', name: 'Torro Chat', short: 'Chat', cls: 'torro-chat', bin: 'torrochat',
    status: 'catalog', family: 'people', icon: 'message-circle', hue: { dark: '#9BD04F', light: '#5E8A12' },
    q: 'Who is there to talk to?',
    s: 'Your colleagues on Nextcloud Talk: who is online, a window per chat, files and quick replies.',
    ver: '0.2.0', size: '24.6 MB', cardState: 'available',
    welcome: {
      headline: 'Colleagues, one click away', sub: 'See who is online, open a window per conversation and answer quickly from anywhere.',
      actions: [
        { icon: 'cloud', t: 'Connect Nextcloud Talk', d: 'Your contacts and rooms appear' },
        { icon: 'users', t: 'Start a group chat', d: 'Pick people, name the room' },
        { icon: 'phone', t: 'Call someone', d: 'Audio and video, in a small window' },
      ],
      peek: { title: 'Online', rows: [['Anna Berg', 'In a meeting', ''], ['Jonas Keller', 'Available', ''], ['Design team', '3 new', '']] },
      tips: ['A window per conversation, floating', 'Quick replies from the Notification Hub'],
    },
  },
  {
    id: 'contacts', name: 'Torro Contacts', short: 'Contacts', cls: 'torro-contacts', bin: 'torrocontacts',
    status: 'catalog', family: 'people', icon: 'book-user', hue: { dark: '#F2784B', light: '#C2410C' },
    q: 'Who was that again?',
    s: 'Your address books from Nextcloud: contacts by group, town or book, birthdays and favourites.',
    ver: '0.2.0', size: '24.6 MB', cardState: 'available',
    welcome: {
      headline: 'Everyone you know, sorted', sub: 'Address books from Nextcloud, grouped by name, town or book, with a map and birthdays.',
      actions: [
        { icon: 'cloud', t: 'Connect an address book', d: 'CardDAV: Nextcloud, iCloud, Radicale' },
        { icon: 'user-plus', t: 'New contact', d: 'Name, mail, number, place', k: '⌘N' },
        { icon: 'cake', t: 'See upcoming birthdays', d: 'Never forget one again' },
      ],
      peek: { title: 'A', rows: [['Anna Berg', 'Design · Berlin', ''], ['Armin Vogel', 'Support · Hamburg', ''], ['Aylin Demir', 'Sales · Köln', '']] },
      tips: ['A map without any key', 'Favourites stay on top'],
    },
  },
  {
    id: 'workplace', name: 'Torro Workplace', short: 'Control centre', cls: 'torro-workplace', bin: 'torroworkplace ui',
    status: 'host', family: 'core', icon: 'layout-grid', hue: { dark: '#E5484D', light: '#D50C0C' },
    q: 'Where do my windows live?',
    s: 'The core: tray icon, workspaces as templates, accounts, AI and the rights of every app.',
    ver: '0.1.0', size: '', cardState: 'host',
    welcome: {
      headline: 'Your workspace, saved', sub: 'Store the tiles you work in as a template and bring them back with one key.',
      actions: [
        { icon: 'layers', t: 'Save this workspace', d: 'Windows, sizes and apps as a template', k: '⌥S' },
        { icon: 'cloud', t: 'Add an account', d: 'Nextcloud, mail, AI and more' },
        { icon: 'layout-grid', t: 'Install more apps', d: 'From the catalog, with one click' },
      ],
      peek: { title: 'Workspaces', rows: [['Morning', 'Mail · Calendar · Tasks', '⌥1'], ['Deep work', 'Assistant · Notes', '⌥2'], ['Support', 'Chat · Feed · Hub', '⌥3']] },
      tips: ['The tray menu always opens the control centre', 'Apps run on their own, too'],
    },
  },
  // ---- geplant: Konzepte und Entwürfe, noch nicht im Katalog ----
  {
    id: 'notes', name: 'Torro Notes', short: 'Notes', cls: 'torro-notes', bin: 'torronotes',
    status: 'planned', family: 'soon', icon: 'file-pen-line', hue: { dark: '#C9B38A', light: '#8A7447' },
    q: 'Where do I jot this down?', s: 'Markdown notes in your Nextcloud Notes folder, offline-first and open to agents.',
    ver: '', size: '', cardState: 'planned', src: 'docs/notizen.md, docs/tasks/notizen.md',
    welcome: {
      headline: 'Write it down, find it later', sub: 'Plain Markdown files in your Nextcloud, readable by you and, if you allow, by your agents.',
      actions: [
        { icon: 'cloud', t: 'Choose a notes folder', d: 'Nextcloud Notes or a local folder' },
        { icon: 'file-pen-line', t: 'New note', d: 'Starts typing right away', k: '⌘N' },
        { icon: 'search', t: 'Search everything', d: 'Titles, text and tags' },
      ],
      peek: { title: 'Recent', rows: [['Meeting notes · 21 Nov', 'Meetings', 'Today'], ['Ideas for the gallery', 'Design', 'Yesterday'], ['Shopping list', 'Home', 'Mon']] },
      tips: ['Notes are plain files', 'Pin a note to the board'],
    },
  },
  {
    id: 'photos', name: 'Torro Photos', short: 'Photos', cls: 'torro-photos', bin: 'torrophotos',
    status: 'planned', family: 'soon', icon: 'image', hue: { dark: '#E879F9', light: '#A21CAF' },
    q: 'What did we shoot?', s: 'Timeline, albums, people and places from your Nextcloud, with nothing leaving it.',
    ver: '', size: '', cardState: 'planned', src: 'docs/fotos.md, docs/tasks/fotos.md',
    welcome: {
      headline: 'Your photos, on your cloud', sub: 'A timeline of everything on Nextcloud, with albums, places and favourites.',
      actions: [
        { icon: 'cloud', t: 'Pick photo folders', d: 'Which folders count as photos' },
        { icon: 'layers', t: 'Create an album', d: 'A link, not a copy' },
        { icon: 'map-pin', t: 'Browse by place', d: 'From the GPS data in your files' },
      ],
      peek: { title: 'November 2026', rows: [['Harbour at dusk', '14 photos', '21 Nov'], ['Team day', '52 photos', '15 Nov'], ['Autumn walk', '9 photos', '2 Nov']] },
      tips: ['Favourites sync as a flag', 'Show an album on a tile'],
    },
  },
  {
    id: 'frame', name: 'Torro Frame', short: 'Frame', cls: 'torro-frame', bin: 'torroframe',
    status: 'planned', family: 'soon', icon: 'presentation', hue: { dark: '#F0A6FB', light: '#A21CAF' },
    q: 'What should hang on the wall?', s: 'A photo or slideshow on a tile of your workspace, for example one album.',
    ver: '', size: '', cardState: 'planned', src: 'docs/fotos.md',
    welcome: {
      headline: 'A picture on your tile', sub: 'Pick a photo or an album and let it fill a tile of your workspace, quietly.',
      actions: [
        { icon: 'image', t: 'Choose an album', d: 'From Torro Photos or a folder' },
        { icon: 'play', t: 'Set the pace', d: 'From every minute to once a day' },
        { icon: 'layers', t: 'Save with the workspace', d: 'It comes back with the template' },
      ],
      peek: { title: 'Now showing', rows: [['Autumn walk', 'Photo 3 of 9', 'Next in 4 min'], ['Harbour at dusk', 'Album', ''], ['Team day', 'Album', '']] },
      tips: ['No controls on the tile', 'Dark mode dims the picture'],
    },
  },
  {
    id: 'agents', name: 'Torro Agents', short: 'Agents', cls: 'torro-agents', bin: 'torroagents',
    status: 'planned', family: 'soon', icon: 'bot', hue: { dark: '#8FA3C7', light: '#5B6F96' },
    q: 'Which agents are running?', s: 'Claude Code and Codex sessions: where they run and who is waiting for you.',
    ver: '', size: '', cardState: 'planned', src: 'docs/app-catalog.md („Agenten-Übersicht“, nächste)',
    welcome: {
      headline: 'Every agent, one overview', sub: 'See which sessions run, on which branch, and which of them is waiting for you.',
      actions: [
        { icon: 'bot', t: 'Connect Claude Code', d: 'Hooks report each session' },
        { icon: 'git-branch', t: 'Show worktrees', d: 'Branch, ticket and plan per agent' },
        { icon: 'chart-column', t: 'Watch usage', d: 'Tokens today and this month' },
      ],
      peek: { title: 'Running', rows: [['feature/970-gallery', 'Waiting for you', '2 min'], ['fix/mail-badge', 'Working', '14 min'], ['docs/api', 'Done', '1 h']] },
      tips: ['A waiting agent also lands in the Hub', 'Usage stays on your machine'],
    },
  },
  {
    id: 'github', name: 'Torro GitHub', short: 'GitHub', cls: 'torro-github', bin: 'torrogithub',
    status: 'planned', family: 'soon', icon: 'git-pull-request', hue: { dark: '#A1A6B2', light: '#565B66' },
    q: 'What needs me on GitHub?', s: 'Pull requests, reviews, CI runs and notifications that need you, in one dashboard.',
    ver: '', size: '', cardState: 'planned', src: 'docs/app-catalog.md („GitHub-Dashboard“, nächste)',
    welcome: {
      headline: 'Reviews and checks, at a glance', sub: 'The pull requests, reviews and CI runs that need you, without opening a browser.',
      actions: [
        { icon: 'github', t: 'Connect with gh', d: 'Uses your existing token' },
        { icon: 'eye', t: 'Show review requests', d: 'Newest first, with checks' },
        { icon: 'git-merge', t: 'Follow a repository', d: 'Pick the ones that matter' },
      ],
      peek: { title: 'Needs you', rows: [['#970 App gallery study', 'Review requested', 'checks passing'], ['#962 Chat player', 'Changes requested', '2 comments'], ['#958 hold_look helper', 'Merged', 'yesterday']] },
      tips: ['Merged is violet, a mark only', 'Checks show as icons'],
    },
  },
  {
    id: 'pinboard', name: 'Torro Pinboard', short: 'Pinboard', cls: 'torro-pinboard', bin: 'torropinboard',
    status: 'parked', family: 'soon', icon: 'pin', hue: { dark: '#C48A6A', light: '#8F5A3A' },
    q: 'What should everyone see?', s: 'A calm board next to your work: notes, checklists and links, placed freely.',
    ver: '', size: '', cardState: 'planned', src: 'Issue #348, #367 (zurückgestellt am 30.10.2026)',
    welcome: {
      headline: 'A board for what is short-lived', sub: 'Snippets, checklists and links, pinned where you see them. Agents can pin to it, too.',
      actions: [
        { icon: 'pin', t: 'Pin a first note', d: 'Text, checklist, link or file', k: '⌘N' },
        { icon: 'layout-grid', t: 'Create a board', d: 'One per project or per day' },
        { icon: 'bot', t: 'Let an agent pin', d: '“PR #341 is waiting for you”' },
      ],
      peek: { title: 'Board “This week”', rows: [['Call the printer', 'Checklist', ''], ['Offer for Müller', 'Link', ''], ['Wifi code', 'Text', '']] },
      tips: ['Colour is the category', 'Pins snap to an 8 px grid'],
    },
  },
];

/** Ideen und fremde Produkte ohne eigenen Entwurf (nur im Inventar). */
export const IDEAS = [
  ['Tagesplan', 'Termine und Aufgaben des Tages zusammen', 'Idee (docs/app-catalog.md)'],
  ['Fokus / Pomodoro / Zeiterfassung', 'Woran arbeite ich gerade, und wie lange schon?', 'Idee'],
  ['Verbrauch & Kosten', 'Tokens und Kosten der Agenten, Nähe zum Limit', 'Idee'],
  ['Kontext-Board', 'Woran arbeiten die Agenten (Branch, Ticket, Plan)?', 'Idee'],
  ['Deploy- und Server-Status', 'Läuft alles? Health-Checks, Hetzner, Monitoring', 'Idee'],
  ['TorroMail (eigenes Produkt)', 'Bestehende App; „in die Suite aufnehmen“. Die Mail der Suite ist Torro Mail oben.', 'Entscheidung offen'],
  ['TorroCast, TorroEQ (eigene Produkte)', 'Bestehende Apps; „in die Suite und den Installer aufnehmen“', 'Entscheidung offen'],
];

// Reihenfolge wie im Katalog der Steuerzentrale (AppId::ALL: Workplace, Hub, Mail, Feed, Tasks, Calendar, ...; die Übersicht
// zeigt installierte Apps zuerst) – hier fest, damit alle Layouts dieselbe Reihenfolge haben.
const ORDER = ['calendar', 'tasks', 'mail', 'hub', 'feed', 'assistant', 'chat', 'contacts', 'workplace'];
APPS.sort((x, y) => (ORDER.indexOf(x.id) + 1 || 99) - (ORDER.indexOf(y.id) + 1 || 99));
export const byId = Object.fromEntries(APPS.map((a) => [a.id, a]));
export const hueOf = (app, T) => app.hue[T.name];
export const real = APPS.filter((a) => a.status === 'catalog');
export const planned = APPS.filter((a) => a.status === 'planned' || a.status === 'parked');
