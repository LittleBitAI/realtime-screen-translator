// P12: runs on every page the app's reader opens. It finds the manga page
// pictures, has the app translate each one once (`post_reader_page`) and
// draws the Korean on the picture itself: the shapes sit in the picture's own
// box, so they scroll, slide and zoom with it, and nothing has to follow the
// screen. An imported chapter uses the same overlays on app-owned images.
(() => {
  if (window.__rstReader || !window.__TAURI_INTERNALS__) return;
  window.__rstReader = true;
  // One window: a pop-up or a link to another site in a new tab would
  // replace the manga page itself. Ad scripts open them on the first tap.
  window.open = () => null;
  addEventListener(
    'click',
    (e) => {
      const a = e.target instanceof Element ? e.target.closest('a[target]') : null;
      if (a && a.target !== '_self' && a.href && new URL(a.href, location.href).host !== location.host) e.preventDefault();
    },
    true,
  );
  const invoke = (cmd, args) => window.__TAURI_INTERNALS__.invoke(cmd, args);
  const MIN_SIDE = 300; // natural pixels: smaller pictures are icons, not pages
  const NEAR = 1.5; // screens above and below the view that are read ahead
  const PARALLEL = 2;
  const done = new Map(); // picture address -> answer, for pictures a site recreates
  const pics = new Map(); // img -> { src, layer, state }
  const queue = new Set();
  let running = 0;
  let on = true;
  let bar;
  let barText;
  let problem = null;
  let chapter = false;

  const messages = {
    key_missing: '설정에서 Gemini API 키를 넣어 주세요.',
    key_invalid: 'Gemini API 키가 맞지 않습니다. 설정에서 확인해 주세요.',
    gemini_unreachable: 'Gemini에 닿지 못했습니다. 인터넷 연결을 확인해 주세요.',
    gemini_unreadable: 'Gemini 답을 읽지 못했습니다. 그림을 눌러 다시 번역합니다.',
    image_unreachable: '이 사이트가 그림을 내주지 않습니다.',
    bad_image: '읽을 수 없는 그림입니다.',
    image_dimensions_too_large: '이 그림은 가로와 세로를 곱해 8,388,608픽셀이 넘어서 번역할 수 없습니다.',
    not_found: '저장된 만화나 그림을 찾지 못했습니다.',
    storage_full: '기기 저장 공간이 모자라 번역본에 담지 못했습니다. 공간을 비우고 그림 위 버튼을 눌러 주세요.',
    storage_denied: '앱이 저장 폴더에 쓰지 못해 번역본에 담지 못했습니다.',
    internal: '번역본에 담지 못했습니다. 그림 위 버튼을 눌러 다시 저장합니다.',
  };

  function start() {
    if (document.querySelector('meta[name="rst-app"]')) {
      chapter = Boolean(document.querySelector('meta[name="rst-chapter"]'));
      if (!chapter) return;
    }
    const css = document.createElement('style');
    css.textContent = `
      .rst-layer{position:absolute;pointer-events:none;z-index:2147483000;transform-origin:0 0}
      html.rst-off .rst-layer{display:none}
      .rst-box{position:absolute;box-sizing:border-box;display:flex;align-items:center;justify-content:center;
        background:#fff;color:#111;text-align:center;line-height:1.2;font-family:system-ui,'Noto Sans KR',sans-serif;
        font-weight:600;word-break:keep-all;overflow-wrap:anywhere;white-space:pre-line;overflow:hidden;pointer-events:auto}
      .rst-box.rst-peek{opacity:.12}
      .rst-chip{position:absolute;left:6px;top:6px;padding:4px 8px;border-radius:12px;background:rgba(17,24,39,.85);
        color:#fff;font:600 12px system-ui,sans-serif;pointer-events:auto}
      .rst-bar{position:fixed;left:12px;bottom:calc(12px + env(safe-area-inset-bottom));z-index:2147483646;display:flex;
        gap:6px;align-items:center;padding:6px;border-radius:24px;background:rgba(17,24,39,.88);color:#fff;
        font:600 14px system-ui,sans-serif;box-shadow:0 2px 8px rgba(0,0,0,.3)}
      .rst-bar button{min-height:44px;min-width:44px;padding:0 14px;border:0;border-radius:20px;background:#fff;color:#111;font:inherit}
      .rst-bar button[aria-pressed=false]{background:#4b5563;color:#fff}
      .rst-bar span{padding:0 6px;max-width:46vw;font-size:12px;font-weight:500}`;
    document.head.appendChild(css);
    bar = document.createElement('div');
    bar.className = 'rst-bar';
    const toggle = document.createElement('button');
    toggle.type = 'button';
    toggle.setAttribute('aria-pressed', 'true');
    toggle.textContent = '번역 켜짐';
    toggle.onclick = () => {
      on = !on;
      toggle.setAttribute('aria-pressed', String(on));
      toggle.textContent = on ? '번역 켜짐' : '번역 꺼짐';
      document.documentElement.classList.toggle('rst-off', !on);
      if (on) scan();
    };
    const back = document.createElement('button');
    back.type = 'button';
    back.textContent = '앱';
    back.setAttribute('aria-label', '앱으로 돌아가기');
    back.onclick = () => chapter ? location.assign('/library') : invoke('post_reader_close', {}).catch(() => history.back());
    barText = document.createElement('span');
    barText.setAttribute('role', 'status');
    barText.hidden = true;
    bar.append(back, toggle, barText);
    document.body.appendChild(bar);
    new MutationObserver(() => schedule()).observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ['src', 'srcset', 'style', 'class'] });
    addEventListener('scroll', schedule, { passive: true, capture: true });
    addEventListener('resize', schedule);
    setInterval(scan, 800);
    if (chapter) loadChapter();
    else scan();
  }

  async function loadChapter() {
    const book_id = Number(new URLSearchParams(location.search).get('id'));
    try {
      if (!Number.isSafeInteger(book_id) || book_id <= 0) throw 'not_found';
      const { title, pages } = await invoke('get_chapter', { book_id });
      document.title = document.querySelector('h1').textContent = title;
      const content = document.createDocumentFragment();
      for (const [i, page] of pages.entries()) {
        const frame = document.createElement('div');
        frame.style.aspectRatio = `${page.width} / ${page.height}`;
        const img = document.createElement('img');
        img.dataset.pictureId = page.id;
        img.alt = `${i + 1}쪽`;
        img.addEventListener('load', schedule);
        frame.appendChild(img);
        content.appendChild(frame);
      }
      document.querySelector('main').appendChild(content);
      scan();
    } catch (e) {
      document.querySelector('h1').textContent = messages[e] ?? messages.not_found;
    }
  }

  async function loadPicture(img, p) {
    p.loading = true;
    try {
      const bytes = await invoke('get_chapter_image', { picture_id: p.src });
      if (pics.get(img) !== p || distance(img) > NEAR * 2) return;
      img.src = URL.createObjectURL(new Blob([new Uint8Array(bytes)]));
    } catch {
      if (pics.get(img) !== p) return;
      p.imageError = true;
      chip(img, p, '그림을 읽을 수 없습니다 · 눌러서 다시', () => {
        p.imageError = false;
        schedule();
      });
    } finally {
      p.loading = false;
    }
  }

  let pending = false;
  function schedule() {
    if (pending) return;
    pending = true;
    requestAnimationFrame(() => {
      pending = false;
      scan();
    });
  }

  function isPage(img) {
    return img.complete && img.naturalWidth >= MIN_SIDE && img.naturalHeight >= MIN_SIDE && img.offsetWidth >= 120 && !img.closest('.rst-layer');
  }

  // How far the picture is from the view, in screens (0 = on screen).
  function distance(img) {
    const r = img.getBoundingClientRect();
    const h = innerHeight || 1;
    const w = innerWidth || 1;
    const dy = r.bottom < 0 ? -r.bottom / h : r.top > h ? (r.top - h) / h : 0;
    const dx = r.right < 0 ? -r.right / w : r.left > w ? (r.left - w) / w : 0;
    return Math.max(dy, dx);
  }

  function scan() {
    for (const [img, p] of pics) {
      if (!img.isConnected || (!chapter && (img.currentSrc || img.src) !== p.src)) forget(img);
    }
    status();
    if (!on && !chapter) return;
    for (const img of document.images) {
      if (chapter ? !img.dataset.pictureId : !isPage(img)) continue;
      const src = chapter ? Number(img.dataset.pictureId) : img.currentSrc || img.src;
      const near = distance(img);
      let p = pics.get(img);
      if (!p) {
        p = { src, layer: null, state: 'new' };
        pics.set(img, p);
        const answer = done.get(src);
        if (answer) {
          p.state = 'done';
          draw(img, p, answer);
        }
      }
      if (p.layer) place(img, p.layer);
      if (chapter) {
        const loaded = img.hasAttribute('src');
        if (near <= NEAR && !loaded && !p.loading && !p.imageError) loadPicture(img, p);
        if (near > NEAR * 2 && loaded) unloadPicture(img);
        if (!loaded || !img.complete || !img.naturalWidth) continue;
      }
      if (on && p.state === 'new' && near <= NEAR) queue.add(img);
    }
    pump();
  }

  function forget(img) {
    const p = pics.get(img);
    p?.layer?.remove();
    pics.delete(img);
    queue.delete(img);
    if (chapter && img.hasAttribute('src')) unloadPicture(img);
  }

  function unloadPicture(img) {
    URL.revokeObjectURL(img.src);
    img.removeAttribute('src');
  }

  function pump() {
    while (running < PARALLEL && queue.size && on) {
      // The picture nearest the view first.
      const img = [...queue].sort((a, b) => distance(a) - distance(b))[0];
      queue.delete(img);
      const p = pics.get(img);
      if (!p || p.state !== 'new' || distance(img) > NEAR) continue;
      translate(img, p);
    }
    status();
  }

  async function bytes(src) {
    try {
      const res = await fetch(src);
      if (!res.ok) return null;
      let blob = await res.blob();
      if (!/^image\/(jpeg|png|webp)$/.test(blob.type)) {
        // AVIF, GIF and the like go as PNG.
        const bmp = await createImageBitmap(blob);
        const c = document.createElement('canvas');
        c.width = bmp.width;
        c.height = bmp.height;
        c.getContext('2d').drawImage(bmp, 0, 0);
        blob = await new Promise((ok) => c.toBlob(ok, 'image/png'));
      }
      const data = await new Promise((ok, fail) => {
        const r = new FileReader();
        r.onload = () => ok(String(r.result).split(',')[1]);
        r.onerror = fail;
        r.readAsDataURL(blob);
      });
      return data;
    } catch {
      return null; // CORS and the like: the app fetches the address itself
    }
  }

  function order(img) {
    const slide = img.closest('.swiper-slide, [data-index], [data-page]');
    if (slide?.parentElement) return [...slide.parentElement.children].indexOf(slide);
    return [...document.images].indexOf(img);
  }

  async function translate(img, p) {
    running++;
    p.state = 'busy';
    chip(img, p, '번역 중…');
    try {
      const answer = chapter
        ? await invoke('post_chapter_page', { picture_id: p.src })
        : await invoke('post_reader_page', { src: p.src, image: await bytes(p.src), page_url: location.href, title: document.title, index: order(img) });
      p.state = 'done';
      problem = answer.save_error ? messages[answer.save_error] ?? messages.storage_full : null;
      // Not kept in the library: drawn, but asked again so the app saves it.
      if (!answer.save_error) done.set(p.src, answer);
      if (pics.get(img) === p) {
        draw(img, p, answer);
        if (answer.save_error) {
          chip(img, p, '번역본에 저장 못 함 · 눌러서 다시 저장', () => {
            p.state = 'new';
            queue.add(img);
            pump();
          });
        }
      }
    } catch (e) {
      const code = typeof e === 'string' ? e : 'gemini_unreachable';
      // A garbled answer or a dropped connection usually passes the second time.
      if (!p.retried && (code === 'gemini_unreadable' || code === 'gemini_unreachable')) {
        p.retried = true;
        p.state = 'new';
        queue.add(img);
        return;
      }
      p.state = 'failed';
      problem = messages[code] ?? messages.gemini_unreachable;
      if (pics.get(img) === p) {
        chip(img, p, '번역 실패 · 눌러서 다시', () => {
          p.state = 'new';
          queue.add(img);
          pump();
        });
      }
    } finally {
      running--;
      pump();
    }
  }

  // The layer covers the picture's own box, in the picture's parent, so it
  // moves with whatever moves the picture.
  function layerFor(img, p) {
    if (!p.layer) {
      p.layer = document.createElement('div');
      p.layer.className = 'rst-layer';
      img.insertAdjacentElement('afterend', p.layer);
    }
    place(img, p.layer);
    return p.layer;
  }

  function place(img, layer) {
    const w = img.offsetWidth;
    const h = img.offsetHeight;
    const key = `${img.offsetLeft},${img.offsetTop},${w},${h}`;
    if (layer.dataset.at === key) return;
    layer.style.left = '0px';
    layer.style.top = '0px';
    layer.style.width = `${w}px`;
    layer.style.height = `${h}px`;
    const lr = layer.getBoundingClientRect();
    const ir = img.getBoundingClientRect();
    const scale = lr.width / (w || 1) || 1;
    layer.style.left = `${(ir.left - lr.left) / scale}px`;
    layer.style.top = `${(ir.top - lr.top) / scale}px`;
    layer.dataset.at = key;
    for (const box of layer.querySelectorAll('.rst-box')) fitFont(box, w);
  }

  function chip(img, p, text, retry) {
    const layer = layerFor(img, p);
    layer.querySelector('.rst-chip')?.remove();
    const c = document.createElement('div');
    c.className = 'rst-chip';
    c.textContent = text;
    if (retry) c.onclick = (e) => {
      e.stopPropagation();
      c.remove();
      retry();
    };
    layer.appendChild(c);
  }

  function draw(img, p, answer) {
    const layer = layerFor(img, p);
    layer.replaceChildren();
    const pct = (v, of) => `${(v / of) * 100}%`;
    for (const b of answer.boxes) {
      const box = document.createElement('div');
      box.className = 'rst-box';
      box.style.left = pct(b.x, answer.width);
      box.style.top = pct(b.y, answer.height);
      box.style.width = pct(b.w, answer.width);
      box.style.height = box.dataset.h = pct(b.h, answer.height);
      box.dataset.font = String(b.font_px / answer.width);
      box.dataset.radius = String(b.radius / answer.width);
      box.textContent = b.ko;
      // A tap shows the original under it for a moment.
      box.onclick = () => box.classList.toggle('rst-peek');
      layer.appendChild(box);
      fitFont(box, img.offsetWidth);
    }
  }

  // The size the app fitted the Korean at, scaled to the picture as shown;
  // never below a size a phone can still read. The browser breaks lines its
  // own way, so where its lines overflow the shape the size steps down.
  function fitFont(box, shownWidth) {
    box.style.borderRadius = `${Number(box.dataset.radius) * shownWidth}px`;
    box.style.height = box.dataset.h;
    box.style.minHeight = '';
    let px = Math.max(7, Number(box.dataset.font) * shownWidth);
    for (;;) {
      box.style.fontSize = `${px}px`;
      box.style.padding = `${Math.max(1, px * 0.1)}px`;
      const over = box.scrollHeight > box.clientHeight + 1 || box.scrollWidth > box.clientWidth + 1;
      if (!over) break;
      if (px <= 7) {
        // Still too long at the smallest size: the shape grows down instead
        // of cutting off the first line.
        box.style.height = 'auto';
        box.style.minHeight = box.dataset.h;
        break;
      }
      px = Math.max(7, px * 0.9);
    }
  }

  function status() {
    if (!barText) return;
    const busy = [...pics.values()].filter((p) => p.state === 'busy').length + queue.size;
    const message = !on ? '' : problem ?? (busy ? `번역 중 ${busy}` : '');
    if (barText.textContent !== message) {
      barText.textContent = message;
      barText.hidden = !message;
    }
  }

  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', start, { once: true });
  else start();
})();
