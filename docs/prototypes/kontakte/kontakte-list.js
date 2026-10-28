/* Gemeinsame Bausteine der Listenentwürfe D1–D3 (nur Entwurf). Braucht kontakte.js und icons.js. */
const Q=new URLSearchParams(location.search), OPEN=Q.get('open')||'', MAP_ON=Q.get('map')==='on';
const asc=s=>s.normalize('NFD').replace(/[̀-ͯ]/g,'').replace(/ß/g,'ss').toLowerCase();
const fakeMail=c=>c.o?`${asc(firstNames(c.n).split(' ')[0])}@${asc(lastName(c.n))}.example`:`${asc(firstNames(c.n))}@mailbox.example`;
const fakePhone=c=>'+49 '+(30+(c.n.length*7)%60)+' 5550 '+(1000+([...c.n].reduce((a,ch)=>a+ch.charCodeAt(0),0)*13)%9000);
const bookOf=c=>BOOKS[c.b];
const SELECTED=CONTACTS.find(c=>c.sel);

/* Ansichtsschalter: führt zu den drei Darstellungen; die Karte erscheint nur, wenn die Ortssuche freigegeben ist (?map=on). */
function viewSeg(active){
  const qs=new URLSearchParams(location.search); qs.delete('open'); const tail=qs.toString()?'?'+qs:'';
  const it=(id,href,icon,label)=>`<a class="${active===id?'on':''}" href="${href}${tail}" title="${label}">${ic(icon,'s14')}<span class="word">${label}</span></a>`;
  return `<div class="seg">${it('table','d1-tabelle.html','table','Table')}${it('groups','d2-gruppen.html','list-tree','Groups')}${it('index','d3-verzeichnis.html','book-user','Index')}${MAP_ON?it('map','c-karte.html','map','Map'):''}</div>`;
}
function header(extra){
  return `<div class="header"><button class="btn only-ms">${ic('users')}<span class="word">All contacts</span>${ic('chevron-down','s12')}</button>
    <div class="crumbs not-ms">${ic('users','t3')}<b>All contacts</b><span class="t3 tab">248</span></div><span class="grow"></span>
    ${viewSeg(extra.view)}
    <button class="btn ghost not-s">${ic('sliders-horizontal')}<span class="word">Display</span></button>
    <button class="btn primary">${ic('plus')}<span class="word">New contact</span></button></div>`;
}
function tools(extra){
  return `<div class="tools"><div class="field">${ic('search')}<input placeholder="Search name, company, phone, city…"><kbd>/</kbd></div>
    <span class="chip on not-s">Everyone <span class="n">248</span></span><span class="chip not-s">${ic('star','s14')}Favorites <span class="n">5</span></span><span class="chip not-ms">Clients <span class="n">18</span></span><span class="chip not-ms">Family <span class="n">6</span></span><span class="chip not-ms">Neighbours <span class="n">4</span></span>
    <span class="grow"></span>${extra||''}</div>`;
}
const star=c=>c.f?`<span class="st" title="Favorite">${ic('star','s14')}</span>`:'';
const lock=c=>c.b===1?`<span class="t3" title="Read-only book">${ic('lock','s12')}</span>`:'';
const gpills=c=>c.g.map(g=>`<span class="gp"><span class="dot" style="--c:${g==='Clients'?'var(--c0)':g==='Family'?'var(--c2)':'var(--c1)'}"></span>${g}</span>`).join('');
const actions=`<div class="ra"><button class="iconbtn" title="Write an email">${ic('mail')}</button><button class="iconbtn" title="Call">${ic('phone')}</button><button class="iconbtn" title="Copy">${ic('copy')}</button></div>`;

/* Kontakt im Seitenpaneel */
function contactPanel(c){
  return `<aside class="cpanel"><div class="ph"><span class="grow strong ell">Contact</span><button class="btn ghost sm">${ic('square-pen','s14')}Edit</button><button class="iconbtn">${ic('x')}</button></div>
  <div class="pb"><div style="display:flex;gap:16px;align-items:center">${av(c,64)}<div class="grow"><h2>${c.n}</h2><div class="t2">${[c.t,c.o].filter(Boolean).join(' · ')}</div></div></div>
    <div class="mini"><button class="btn">${ic('mail')}Mail</button><button class="btn">${ic('phone')}Call</button><button class="btn">${ic('map-pin')}Map</button><button class="btn sq">${ic('ellipsis')}</button></div>
    <div class="sect"><div class="cap">Phone</div><div class="pl"><span class="lab">mobile</span><span class="tab">${fakePhone(c)}</span></div></div>
    <div class="sect"><div class="cap">Email</div><div class="pl"><span class="lab">work</span><span>${fakeMail(c)}</span></div></div>
    <div class="sect"><div class="cap">Address</div><div class="pl"><span class="lab">work</span><span>Kastanienallee 42, ${c.city}</span></div></div>
    <div class="sect"><div class="cap">Groups</div><div style="display:flex;gap:6px;flex-wrap:wrap">${gpills(c)||''}<span class="chip" style="border-style:dashed;color:var(--text3)">${ic('plus','s14')}Add</span></div></div>
    <div class="sect"><div class="cap">Connected in Torro</div><div class="pl"><span>${ic('mail','t3')}</span><span class="grow ell">Re: Cover proofs for the spring list</span><span class="t3 meta">Tue</span></div><div class="pl"><span>${ic('calendar-days','t3')}</span><span class="grow ell">Lunch</span><span class="t3 meta">Thu 12:30</span></div></div>
    <div class="meta t3" style="margin-top:24px;display:flex;gap:6px;align-items:center">${ic('refresh-cw','s12')}Synced with Nextcloud · ${bookOf(c).name}</div>
  </div></aside>`;
}

/* Fenster zusammenbauen: page(view, viewHtml, toolsExtra) */
function mount(view,viewHtml,toolsExtra,overlay){
  document.getElementById('app').innerHTML=`${sidebar('all',{map:MAP_ON})}<main class="main">${header({view})}
    <div class="body"><section class="view">${tools(toolsExtra)}${viewHtml}</section>${OPEN==='panel'?contactPanel(SELECTED):''}</div>${overlay||''}</main>`;
  paintIcons();
}
