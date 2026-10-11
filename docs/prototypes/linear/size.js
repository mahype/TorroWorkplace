// Größenklasse und Entwurfszustand aus der Fenstergröße bzw. der URL (?theme=light, ?size=m, ?open=menu).
(function(){
  const q=new URLSearchParams(location.search), d=document.documentElement, w=innerWidth;
  if(q.get('theme')==='light')d.dataset.theme='light';
  d.dataset.size=q.get('size')||(w>=1024?'l':w>=600?'m':'s');
  d.dataset.open=q.get('open')||'';
})();
