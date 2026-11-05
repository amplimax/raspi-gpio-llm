'use strict';
const sourceData = JSON.parse(document.getElementById('source-data').textContent);
const $ = id => document.getElementById(id);
const esc = value => String(value).replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const norm = value => value.toLowerCase().normalize('NFD').replace(/[\u0300-\u036f]/g, '');
let currentFile = Object.keys(sourceData)[0], currentMatches = [], matchIndex = -1;
let toastTimer;
function toast(message){$('toast').textContent=message;$('toast').classList.add('visible');clearTimeout(toastTimer);toastTimer=setTimeout(()=>$('toast').classList.remove('visible'),3000);}
function showDialog(id){const d=$(id);if(!d.open)d.showModal();}
function closeDialog(id){const d=$(id);if(d.open)d.close();}
document.querySelectorAll('[data-close]').forEach(b=>b.addEventListener('click',()=>closeDialog(b.dataset.close)));
document.querySelectorAll('dialog').forEach(d=>d.addEventListener('click',e=>{if(e.target===d){const r=d.getBoundingClientRect();if(e.clientX<r.left||e.clientX>r.right||e.clientY<r.top||e.clientY>r.bottom)d.close();}}));
const fileSelect=$('source-select');
Object.keys(sourceData).forEach(file=>{const o=document.createElement('option');o.value=file;o.textContent=file;fileSelect.append(o);});
function showFile(file,line=1){
 if(!sourceData[file]){toast('File non presente nello snapshot');return;}
 currentFile=file;fileSelect.value=file;$('source-find').value='';currentMatches=[];matchIndex=-1;$('source-next').textContent='Risultato successivo ↓';
 const source=sourceData[file];
 $('source-meta').textContent=`${source.lines} righe · SHA-256 ${source.sha256}`;
 const lines=source.text.split('\n');if(lines[lines.length-1]==='')lines.pop();
 $('source-code').innerHTML=lines.map((text,i)=>`<div class="code-row" data-line="${i+1}"><span class="line-no">${i+1}</span><span class="line-text">${esc(text)||' '}</span></div>`).join('');
 showDialog('source-dialog');
 const selected=$('source-code').querySelector(`[data-line="${Math.max(1,line)}"]`);
 if(selected){selected.classList.add('selected');$('source-code').scrollTop=Math.max(0,selected.offsetTop-$('source-code').offsetTop-90);}
}
document.addEventListener('click',e=>{const a=e.target.closest('.source-link');if(a){e.preventDefault();showFile(a.dataset.file,Number(a.dataset.line)||1);}});
fileSelect.addEventListener('change',()=>showFile(fileSelect.value));
function findInFile(){const query=norm($('source-find').value);const rows=[...$('source-code').querySelectorAll('.code-row')];currentMatches=[];matchIndex=-1;rows.forEach(row=>{const match=Boolean(query)&&norm(row.querySelector('.line-text').textContent).includes(query);row.classList.toggle('matched',match);if(match)currentMatches.push(row);});if(currentMatches.length)nextMatch();}
function nextMatch(){if(!currentMatches.length){if($('source-find').value)toast('Nessuna corrispondenza nel file');return;}matchIndex=(matchIndex+1)%currentMatches.length;const row=currentMatches[matchIndex];$('source-code').querySelectorAll('.selected').forEach(x=>x.classList.remove('selected'));row.classList.add('selected');$('source-code').scrollTop=Math.max(0,row.offsetTop-$('source-code').offsetTop-90);$('source-next').textContent=`${matchIndex+1}/${currentMatches.length} ↓`;}
$('source-find').addEventListener('input',()=>{findInFile();if(!currentMatches.length)$('source-next').textContent='Risultato successivo ↓';});
$('source-find').addEventListener('keydown',e=>{if(e.key==='Enter'){e.preventDefault();nextMatch();}});$('source-next').addEventListener('click',nextMatch);
async function copyText(text){try{if(navigator.clipboard&&window.isSecureContext){await navigator.clipboard.writeText(text);}else{const ta=document.createElement('textarea');ta.value=text;ta.style.position='fixed';ta.style.opacity='0';const host=document.querySelector('dialog[open]')||document.body;host.append(ta);ta.select();if(!document.execCommand('copy'))throw Error();ta.remove();}toast('Copiato negli appunti');}catch{toast('Copia non disponibile: seleziona il testo e usa Ctrl+C');}}
$('source-copy').addEventListener('click',()=>copyText(sourceData[currentFile].text));
$('browse-code').addEventListener('click',()=>showFile('src/main.rs'));
const chapterIndex=[...document.querySelectorAll('section[data-title]')].map(s=>({id:s.id,title:s.dataset.title,text:s.innerText}));
function openSearch(){showDialog('search-dialog');$('search-input').focus();renderSearch();}
function renderSearch(){const query=norm($('search-input').value.trim());const box=$('search-results');box.replaceChildren();if(!query){box.innerHTML='<p>Cerca un concetto, un nome di funzione o una frase. Esempi: <code>ownership</code>, <code>ready_notify</code>, <code>mezzo</code>.</p>';return;}
 let count=0;
 for(const item of chapterIndex){const n=norm(item.title+' '+item.text);const pos=n.indexOf(query);if(pos<0)continue;const b=document.createElement('button');b.className='search-result';b.type='button';const textpos=norm(item.text).indexOf(query);const start=Math.max(0,textpos-60);b.innerHTML=`<small>CAPITOLO</small><strong>${esc(item.title)}</strong><p>…${esc(item.text.slice(start,start+200).replace(/\s+/g,' '))}…</p>`;b.addEventListener('click',()=>{closeDialog('search-dialog');location.hash=item.id;document.getElementById(item.id).scrollIntoView();});box.append(b);if(++count>=9)break;}
 let sourceCount=0;
 for(const [file,source] of Object.entries(sourceData)){const rows=source.text.split('\n');const line=rows.findIndex(row=>norm(row).includes(query));if(line<0&&!norm(file).includes(query))continue;const b=document.createElement('button');b.className='search-result';b.type='button';b.innerHTML=`<small>SORGENTE · RIGA ${Math.max(1,line+1)}</small><strong>${esc(file)}</strong><p>${line>=0?esc(rows[line].trim().slice(0,190)):esc(file)}</p>`;b.addEventListener('click',()=>{closeDialog('search-dialog');showFile(file,Math.max(1,line+1));});box.append(b);if(++sourceCount>=7)break;}
 if(!count&&!sourceCount)box.innerHTML='<p>Nessuna corrispondenza. Prova un nome più breve.</p>';
}
$('search-open').addEventListener('click',openSearch);$('mobile-search').addEventListener('click',openSearch);$('search-input').addEventListener('input',renderSearch);
document.addEventListener('keydown',e=>{if((e.ctrlKey||e.metaKey)&&e.key.toLowerCase()==='k'){e.preventDefault();if(!$('source-dialog').open&&!$('figure-dialog').open)openSearch();}});
document.querySelectorAll('.figure-button').forEach(b=>b.addEventListener('click',()=>{const small=b.querySelector('img');$('figure-large').src=small.src;$('figure-large').alt=small.alt;showDialog('figure-dialog');}));
$('menu-toggle').addEventListener('click',()=>{const open=$('sidebar').classList.toggle('open');$('menu-toggle').setAttribute('aria-expanded',String(open));});
document.querySelectorAll('.sidebar nav a').forEach(a=>a.addEventListener('click',()=>{$('sidebar').classList.remove('open');$('menu-toggle').setAttribute('aria-expanded','false');}));
const navLinks=[...document.querySelectorAll('.sidebar nav a')];
const sections=[...document.querySelectorAll('section[data-title]')];
let scrollPending=false;
function onScroll(){const viewportTop=window.innerWidth<=850?85:60;let active=sections[0];for(const s of sections){if(s.getBoundingClientRect().top<=viewportTop+80)active=s;else break;}navLinks.forEach(a=>{const selected=a.hash==='#'+active.id;a.classList.toggle('active',selected);if(selected)a.setAttribute('aria-current','location');else a.removeAttribute('aria-current');});const total=document.documentElement.scrollHeight-innerHeight;const available=innerWidth-(innerWidth>850?$('sidebar').offsetWidth:0);$('reading-progress').style.width=`${Math.max(0,Math.min(1,total>0?scrollY/total:0))*available}px`;scrollPending=false;}
addEventListener('scroll',()=>{if(!scrollPending){scrollPending=true;requestAnimationFrame(onScroll);}},{passive:true});addEventListener('resize',onScroll);onScroll();
$('print').addEventListener('click',()=>{document.querySelectorAll('details').forEach(d=>{d.dataset.wasOpen=String(d.open);d.open=true;});window.print();});
addEventListener('afterprint',()=>document.querySelectorAll('details[data-was-open]').forEach(d=>{d.open=d.dataset.wasOpen==='true';delete d.dataset.wasOpen;}));
const examples={
 valid:{text:'lampeggia pin 17 ogni 0.5 secondi',facts:'Numbers { pin: Some(17), interval_s: Some(0.5) }',raw:'Blink { pin: Some(27), interval_s: Some(2.0) }',fixed:'Blink { pin: Some(17), interval_s: Some(0.5) }',result:'Action::Blink { pin: 17, half_period: 500 ms }',status:'Valido → 5 lampeggi completi → avvio del worker GPIO17',note:'La proposta errata del modello viene vincolata ai numeri della trascrizione. Esempio intenzionalmente errato, costruito per spiegare constrain().'},
 missing:{text:'lampeggia led ogni 0.5 secondi',facts:'Numbers { pin: None, interval_s: Some(0.5) }',raw:'Blink { pin: Some(17), interval_s: Some(0.5) }',fixed:'Blink { pin: None, interval_s: Some(0.5) }',result:'ValidationError::MissingPin',status:'Rifiutato → 2 lampeggi → nessuna nuova attuazione',note:'Il pin inventato dal modello viene rimosso. Il validatore richiede un valore esplicito.'},
 reserved:{text:'accendi led sul pin 23',facts:'Numbers { pin: Some(23), interval_s: None }',raw:'On { pin: Some(23) }',fixed:'On { pin: Some(23) }',result:'ValidationError::PinNotAllowed(23)',status:'Rifiutato → 2 lampeggi → nessuna nuova attuazione',note:'BCM23 appartiene al LED di stato. Anche una whitelist personalizzata non lo rende comandabile.'},
 short:{text:'lampeggia pin 17 ogni 0.05 secondi',facts:'Numbers { pin: Some(17), interval_s: Some(0.05) }',raw:'Blink { pin: Some(17), interval_s: Some(0.05) }',fixed:'Blink { pin: Some(17), interval_s: Some(0.05) }',result:'ValidationError::IntervalOutOfRange { min: 0.1, max: 60.0 }',status:'Rifiutato → 2 lampeggi → nessuna nuova attuazione',note:'Una trascrizione e un JSON corretti possono comunque descrivere un’azione fuori policy.'}
};
function renderExample(){const x=examples[$('example-select').value];$('example-text').textContent=x.text;$('example-facts').textContent=x.facts;$('example-raw').textContent=x.raw;$('example-fixed').textContent=x.fixed;$('example-action').textContent=x.result;$('example-status').textContent=x.status;$('example-note').textContent=x.note;}
$('example-select').addEventListener('change',renderExample);renderExample();
