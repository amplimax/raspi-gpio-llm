"""Verify the offline manual with Playwright; never runs application or GPIO code.

Developer prerequisite: Python playwright package and its Chromium browser.
Run from the project root: python scripts/verifica-manuale.py
"""
from datetime import date
from hashlib import sha256
import json
from pathlib import Path
from playwright.sync_api import sync_playwright

root = Path(__file__).resolve().parent.parent
(root / '.work').mkdir(exist_ok=True)
uri = (root / 'docs/manuale-software.html').as_uri()
report = {}
with sync_playwright() as p:
    browser = p.chromium.launch(headless=True)
    page = browser.new_page(viewport={'width': 1512, 'height': 1024}, device_scale_factor=1)
    errors = []
    requests = []
    page.on('pageerror', lambda error: errors.append(str(error)))
    page.on('request', lambda req: requests.append(req.url) if req.url.startswith(('http:', 'https:')) else None)
    page.goto(uri, wait_until='load')
    page.evaluate('Promise.all([...document.querySelectorAll("figure.diagram img")].map(img => {img.loading="eager";return img.decode();}))')
    report['chapters'] = page.locator('section[data-title]').count()
    report['figures'] = page.locator('figure.diagram img').count()
    report['source_links'] = page.locator('a.source-link').count()
    assert report['chapters'] == 20 and report['figures'] == 4
    report['broken_anchors'] = page.evaluate("""() => [...document.querySelectorAll('a[href^="#"]')].filter(a => a.hash && !document.getElementById(decodeURIComponent(a.hash.slice(1)))).map(a=>a.getAttribute('href'))""")
    assert not report['broken_anchors'], report['broken_anchors']
    report['invalid_source_links'] = page.evaluate("""() => [...document.querySelectorAll('a.source-link')].filter(a=>!sourceData[a.dataset.file] || +a.dataset.line<1 || +a.dataset.line>sourceData[a.dataset.file].lines).map(a=>a.outerHTML)""")
    assert not report['invalid_source_links']
    report['source_files'] = page.locator('#source-select option').count()
    assert report['source_files'] == 28
    embedded = page.evaluate('sourceData')
    for filename, source in embedded.items():
        if filename.startswith('deploy/'):
            continue
        raw = (root / filename).read_bytes()
        assert sha256(raw).hexdigest() == source['sha256'], filename
        assert raw.decode('utf-8').replace('\r\n', '\n') == source['text'], filename
    report['source_snapshot_matches_files'] = True
    # Open a function link from the explanatory prose.
    page.locator('#introduzione a.source-link').first.click()
    assert page.locator('#source-dialog').evaluate('(d)=>d.open')
    assert page.locator('#source-select').input_value() == 'src/controller.rs'
    assert 'pub fn prepare_voice' in page.locator('.code-row.selected').inner_text()
    page.locator('#source-find').fill('Outcome')
    assert page.locator('.code-row.matched').count() > 1
    first = page.locator('.code-row.selected').get_attribute('data-line')
    page.locator('#source-next').click()
    assert page.locator('.code-row.selected').get_attribute('data-line') != first
    page.keyboard.press('Escape')
    assert not page.locator('#source-dialog').evaluate('(d)=>d.open')
    page.keyboard.press('Control+k')
    page.locator('#search-input').fill('normalize_command')
    result = page.locator('.search-result').filter(has_text='voice/stt.py').first
    result.click()
    assert page.locator('#source-select').input_value() == 'voice/stt.py'
    page.keyboard.press('Escape')
    page.locator('#example-select').select_option('reserved')
    assert 'PinNotAllowed(23)' in page.locator('#example-action').inner_text()
    page.locator('#example-select').select_option('valid')
    assert 'Some(17)' in page.locator('#example-fixed').inner_text()
    assert 'Some(27)' in page.locator('#example-raw').inner_text()
    page.locator('.figure-button').first.click()
    assert page.locator('#figure-dialog').evaluate('(d)=>d.open')
    assert page.locator('#figure-large').evaluate('(img)=>img.naturalWidth') > 1000
    page.keyboard.press('Escape')
    page.locator('.sidebar nav a[href="#configurazione"]').click()
    page.wait_for_timeout(1300)
    assert page.locator('.sidebar nav a[href="#configurazione"]').get_attribute('aria-current') == 'location'
    page.evaluate('window.scrollTo({top:0,behavior:"instant"})')
    page.wait_for_timeout(300)
    page.screenshot(path=str(root / '.work/manuale-desktop.png'))
    # A source excerpt is readable and scrollable in the dialog.
    page.locator('#introduzione a.source-link').first.click()
    page.screenshot(path=str(root / '.work/manuale-codice.png'))
    page.keyboard.press('Escape')
    page.emulate_media(media='print')
    assert page.locator('.sidebar').is_hidden()
    page.emulate_media(media='screen')
    # Narrow viewport, no horizontal document overflow.
    page.set_viewport_size({'width': 390, 'height': 844})
    page.evaluate('window.scrollTo({top:0,behavior:"instant"})')
    report['mobile_overflow_px'] = page.evaluate('document.documentElement.scrollWidth-innerWidth')
    assert report['mobile_overflow_px'] <= 1
    page.locator('#menu-toggle').click()
    assert page.locator('#sidebar').evaluate('(n)=>n.classList.contains("open")')
    page.locator('.sidebar nav a[href="#moduli"]').click()
    assert not page.locator('#sidebar').evaluate('(n)=>n.classList.contains("open")')
    page.evaluate('window.scrollTo({top:0,behavior:"instant"})')
    page.wait_for_timeout(300)
    page.screenshot(path=str(root / '.work/manuale-mobile.png'))
    report['network_requests'] = requests
    report['javascript_errors'] = errors
    report['resource_dependencies'] = page.evaluate("""() => [...document.querySelectorAll('script[src],link[rel="stylesheet"],img[src]')].filter(n=>n.tagName!=='IMG'||!n.src.startsWith('data:')).map(n=>n.src||n.href)""")
    assert not report['resource_dependencies']
    assert not requests and not errors
    browser.close()
report['verified_on'] = date.today().isoformat()
report['manual_sha256'] = sha256((root / 'docs/manuale-software.html').read_bytes()).hexdigest()
report['browser'] = 'Chromium headless, Playwright; file://; viewport 1512x1024 e 390x844'
report['application_changed'] = False
report['checks'] = ['ricerca capitoli e sorgenti', 'selezione funzione e numero di riga',
                    'ricerca nel file e risultato successivo', 'apertura e chiusura dialoghi',
                    'ingrandimento figure', 'casi didattici numerici', 'indice e stampa CSS']
(root / 'docs/manuale-verifica.json').write_text(json.dumps(report, ensure_ascii=False, indent=2)+'\n',encoding='utf-8')
print(json.dumps(report, ensure_ascii=False, indent=2))
