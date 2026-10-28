/* Gemeinsame Daten und Bausteine der Kontakte-Entwürfe (nur Entwurf, nichts davon ist Produktcode).
   Aufruf-Parameter: ?theme=light  ?size=l|m|s  ?open=<Zustand der jeweiligen Seite> */
(function(){
  const q=new URLSearchParams(location.search), d=document.documentElement, w=innerWidth;
  if(q.get('theme')==='light')d.dataset.theme='light';
  d.dataset.size=q.get('size')||(w>=1024?'l':w>=600?'m':'s');
  d.dataset.open=q.get('open')||'';
})();

/* Beispieldaten: erfunden. b = Adressbuch (0 Contacts, 1 Team directory = schreibgeschützt, 2 Fastmail), g = Gruppen (CATEGORIES),
   bd = Geburtstag, x/y = Lage auf der Entwurfskarte (0–800 × 0–700, Norddeutschland), photo = hat ein Foto. */
const BOOKS=[
  {name:'Contacts',acct:0,c:'var(--c0)',n:212},
  {name:'Team directory',acct:0,c:'var(--c1)',n:54,ro:true},
  {name:'Personal',acct:1,c:'var(--c4)',n:38},
];
const CONTACTS=[
  {n:'Ines Albrecht',o:'Albrecht & Partner',t:'Attorney',city:'Hannover',b:0,g:['Clients'],x:289,y:683,last:'3 weeks ago'},
  {n:'Nils Andersen',o:'Andersen Fisk',t:'Owner',city:'Aalborg',b:0,g:[],x:0,y:0,last:'5 months ago'},
  {n:'Tom Becker',o:'Becker Schlüsseldienst',t:'',city:'Hamburg',b:0,g:['Neighbours'],x:318,y:372,last:'1 year ago'},
  {n:'Felix Brandt',o:'Brandt Medien',t:'Creative director',city:'Lübeck',b:0,g:['Clients'],f:1,photo:['#7A5C4A','#3A2D28','#C9A58C'],x:433,y:307,last:'Yesterday'},
  {n:'Alexander Vogt',o:'Vogt Architekten',t:'Architect',city:'Hamburg',b:0,g:['Clients'],x:345,y:398,last:'2 months ago'},
  {n:'Fatima Yilmaz',o:'Yilmaz Elektro',t:'Electrician',city:'Köln',b:0,g:['Neighbours'],x:0,y:0,last:'4 weeks ago'},
  {n:'Greta Lindholm',o:'Nordlicht Design',t:'Illustrator',city:'Stockholm',b:2,g:[],x:0,y:0,last:'2 months ago'},
  {n:'Hannes Weber',o:'Weber Landtechnik',t:'',city:'Husum',b:0,g:[],x:187,y:155,last:'1 year ago'},
  {n:'Jonas Keller',o:'Keller IT',t:'Sysadmin',city:'Bremen',b:0,g:['Clients'],f:1,x:150,y:505,last:'Today'},
  {n:'Karl-Heinz Petersen',o:'Petersen Werft',t:'Managing director',city:'Flensburg',b:0,g:['Clients'],x:246,y:80,last:'6 months ago'},
  {n:'Amelie Krause',o:'Krause Verlag',t:'Managing editor',city:'Berlin',b:0,g:['Clients'],f:1,photo:['#5C6F82','#26303A','#B7C4D1'],bd:'Mar 12',x:0,y:0,last:'2 days ago',sel:1},
  {n:'Clara Neumann',o:'Neumann Steuerberatung',t:'Tax advisor',city:'Kiel',b:0,g:['Clients'],bd:'Nov 2',x:351,y:195,last:'3 weeks ago'},
  {n:'Lena Fischer',o:'Fischer Fotografie',t:'Photographer',city:'Leipzig',b:0,g:[],photo:['#8A6A5A','#3B2E29','#D8B7A2'],x:0,y:0,last:'Last week'},
  {n:'Lukas Meier',o:'',t:'Brother',city:'Dresden',b:0,g:['Family'],f:1,bd:'Oct 31',x:0,y:0,last:'Last week'},
  {n:'Marta Kowalski',o:'Kowalski Bau',t:'Site manager',city:'Szczecin',b:2,g:[],x:0,y:0,last:'8 months ago'},
  {n:'Miriam Haas',o:'Haas Immobilien',t:'Agent',city:'Rostock',b:0,g:[],x:651,y:252,last:'2 months ago'},
  {n:'Oma Hilde',o:'',t:'Grandmother',city:'Husum',b:0,g:['Family'],f:1,bd:'Nov 8',x:192,y:160,last:'Sunday'},
  {n:'Paul Lehmann',o:'Handwerkskammer Hamburg',t:'Consultant',city:'Hamburg',b:1,g:[],x:330,y:387,last:'3 months ago'},
  {n:'Julia Sommer',o:'Sommer Catering',t:'Owner',city:'Hamburg',b:0,g:['Clients'],x:300,y:395,last:'Last week'},
  {n:'Emil Schröder',o:'Praxis Dr. Schröder',t:'GP',city:'Hamburg',b:0,g:[],x:360,y:380,last:'4 months ago'},
  {n:'Bernd Hoffmann',o:'Stadtwerke Nordheim',t:'Account manager',city:'Lüneburg',b:0,g:['Clients'],x:390,y:462,last:'2 weeks ago'},
  {n:'Sophie Marchand',o:'Atelier Marchand',t:'Ceramicist',city:'Lyon',b:2,g:[],x:0,y:0,last:'1 year ago'},
  {n:'David Okafor',o:'Okafor Logistics',t:'Dispatcher',city:'Schwerin',b:1,g:[],x:540,y:367,last:'3 weeks ago'},
];
const lastName=n=>{const p=n.replace(/^Dr\. /,'').split(' ');return p[p.length-1]};
const firstNames=n=>{const p=n.split(' ');p.pop();return p.join(' ')};
const initials=n=>{const p=n.split(' ');return (p[0][0]+(p.length>1?p[p.length-1][0]:'')).toUpperCase()};
const tint=n=>'var(--c'+([...n].reduce((a,ch)=>a+ch.charCodeAt(0),0)%5)+')';
const byLast=[...CONTACTS].sort((a,b)=>lastName(a.n).localeCompare(lastName(b.n),'de'));
function av(c,size,extra){
  const cls='av s'+size+(c.photo?' photo':'')+(extra?' '+extra:'');
  const st=c.photo?`--p1:${c.photo[0]};--p2:${c.photo[1]};--p3:${c.photo[2]}`:`--c:${tint(c.n)}`;
  return `<span class="${cls}" style="${st}">${c.photo?'':initials(c.n)}</span>`;
}
const ic=(n,cls)=>`<i data-i="${n}" class="${cls||''}"></i>`;

/* Seitenleiste für alle drei Entwürfe. active: all|fav|recent|bday|map|book:<Name>|group:<Name> */
function sidebar(active,opt){
  const mapOn=!(opt&&opt.map===false);
  const nav=(id,icon,label,n,extra)=>`<div class="nav${active===id?' on':''}">${ic(icon)}<span class="grow ell">${label}</span>${extra||''}${n!=null?`<span class="n">${n}</span>`:''}</div>`;
  const book=(b,key)=>`<div class="nav${active==='book:'+b.name?' on':''}"><span class="dot" style="--c:${b.c}"></span><span class="grow ell">${b.name}</span>${b.ro?ic('lock','s12 t3'):''}<span class="n">${b.n}</span></div>`;
  const grp=(name,n)=>`<div class="nav${active==='group:'+name?' on':''}">${ic('tag','s14')}<span class="grow ell">${name}</span><span class="n">${n}</span></div>`;
  return `<aside class="sidebar">
    <div style="display:flex;align-items:center;gap:2px">
      <div class="switcher"><div class="logo">T</div><b class="strong">Contacts</b>${ic('chevron-down','s14 t3')}</div>
      <button class="iconbtn" title="Search">${ic('search')}</button>
      <button class="iconbtn boxed" title="New contact">${ic('user-plus')}</button>
    </div>
    <div style="height:8px"></div>
    ${nav('all','users','All contacts',248)}
    ${nav('fav','star','Favorites',5)}
    ${nav('recent','history','Recent')}
    ${nav('bday','cake','Birthdays',null,'<span class="pip" title="2 in the next 14 days"></span>')}
    ${mapOn?nav('map','map','Map'):''}
    <div class="gt">${ic('chevron-down','s12')}${ic('cloud','s12')}<span class="grow ell">Nextcloud</span></div>
    ${book(BOOKS[0])}
    <div class="child">${grp('Clients',18)}${grp('Family',6)}${grp('Neighbours',4)}</div>
    ${book(BOOKS[1])}
    <div class="gt">${ic('chevron-down','s12')}${ic('cloud','s12')}<span class="grow ell">Fastmail</span></div>
    ${book(BOOKS[2])}
    <div class="spacer"></div>
    <div class="syncrow" data-sync>${ic('refresh-cw','ok s14')}<span class="grow meta">Synced 3 min ago</span><span class="micro t3">CardDAV</span></div>
    <div class="nav">${ic('settings')}<span class="grow">Settings</span></div>
  </aside>`;
}
document.addEventListener('DOMContentLoaded',()=>{paintIcons();});
