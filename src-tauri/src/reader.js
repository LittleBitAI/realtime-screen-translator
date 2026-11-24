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
    // A white rim round each Korean letter: legible where it is set over
    // the art beside lettering that has no balloon.
    const HALO = [...Array(8)].map((_, k) => `${(Math.cos((k * Math.PI) / 4) * 0.14).toFixed(3)}em ${(Math.sin((k * Math.PI) / 4) * 0.14).toFixed(3)}em 0.04em #fff`).join(',');
    const css = document.createElement('style');
    css.textContent = `
      .rst-layer{position:absolute;pointer-events:none;z-index:2147483000;transform-origin:0 0}
      html.rst-off .rst-layer{display:none}
      .rst-box{position:absolute;box-sizing:border-box;pointer-events:auto}
      .rst-fill{position:absolute;inset:0;background:#fff}
      .rst-patch{position:absolute}
      .rst-text{position:absolute;z-index:1;display:flex;align-items:center;justify-content:center;color:#111;text-align:center;
        line-height:1.2;font-family:system-ui,'Noto Sans KR',sans-serif;font-weight:600;word-break:keep-all;
        white-space:pre-line;text-wrap:balance;overflow:hidden;
        text-shadow:${HALO}}
      .rst-text.rst-flow{display:block}
      .rst-side{shape-margin:min(0.5em,6%)}
      .rst-box.rst-peek{opacity:.12}
      .rst-chip{position:absolute;left:6px;top:6px;padding:4px 8px;border-radius:12px;background:rgba(17,24,39,.85);
        color:#fff;font:600 12px system-ui,sans-serif;pointer-events:auto}
      .rst-bar{position:fixed;left:calc(12px + env(safe-area-inset-left));bottom:calc(12px + env(safe-area-inset-bottom));
        max-width:calc(100% - 24px - env(safe-area-inset-left) - env(safe-area-inset-right));box-sizing:border-box;
        z-index:2147483646;display:flex;flex-wrap:wrap;gap:8px;align-items:center;padding:8px;border-radius:16px;background:#111827;color:#fff;
        font:600 14px system-ui,sans-serif;box-shadow:0 2px 8px rgba(0,0,0,.3)}
      .rst-bar button{min-height:48px;min-width:48px;padding:0 12px;border:0;border-radius:8px;background:#fff;color:#111;font:inherit}
      .rst-bar button:focus-visible{outline:2px solid #93c5fd;outline-offset:2px}
      .rst-bar button[aria-pressed=false]{background:#4b5563;color:#fff}
      .rst-bar span{padding:0 6px;max-width:26rem;font-size:14px;font-weight:500;overflow-wrap:anywhere}`;
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
    back.textContent = '앱으로 돌아가기';
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
        // Bytes that are no picture: told, and no longer waited for.
        img.addEventListener('error', () => {
          const p = pics.get(img);
          if (!p || !img.hasAttribute('src')) return;
          unloadPicture(img);
          pictureFailed(img, p);
        });
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
      if (pics.get(img) === p) pictureFailed(img, p);
    } finally {
      p.loading = false;
    }
  }

  // A chapter picture that could not be read or shown: it no longer holds
  // back the pages after it, and a tap reads it again.
  function pictureFailed(img, p) {
    p.imageError = true;
    chip(img, p, '그림을 읽을 수 없습니다 · 눌러서 다시', () => {
      p.imageError = false;
      schedule();
    });
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
    // A page in range whose picture is still loading holds back the pages
    // after it: the app has a page wait only for the earlier ones it has
    // been sent, so its request carries their lines. On a site that is any
    // page-wide picture not yet loaded; one that failed (loaded, no size)
    // holds nothing.
    let held = false;
    for (const img of document.images) {
      if (!chapter && !img.complete) {
        held ||= on && img.offsetWidth >= 120 && !img.closest('.rst-layer') && distance(img) <= NEAR;
        continue;
      }
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
        if (!loaded || !img.complete || !img.naturalWidth) {
          held ||= near <= NEAR && p.state === 'new' && !p.imageError;
          continue;
        }
      }
      if (on && p.state === 'new' && near <= NEAR && !held) queue.add(img);
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

  // One page at a time, the earliest in reading order, each sent only once
  // the one before has been answered (and so kept): a page's request
  // carries the lines of the earlier pages the app has kept.
  function pump() {
    while (!running && queue.size && on) {
      const img = [...queue].sort((a, b) => order(a) - order(b))[0];
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
    // Hidden while the original is shown, the layer has no box to measure
    // from: placed then, it was placed a scroll away (ch256 p15's Korean
    // over p18). It is placed as the translation comes back on.
    if (layer.dataset.at === key || document.documentElement.classList.contains('rst-off')) return;
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
    // Drawn while the original is shown, the Korean has no lines to measure
    // and keeps its first size: it is fitted again as the translation comes
    // back on (ch258 p14 came back cut off).
    if (document.documentElement.classList.contains('rst-off')) delete layer.dataset.at;
    const pct = (v, of) => `${(v / of) * 100}%`;
    for (const b of answer.boxes) {
      // A line kept without a shape: no lettering was found for it.
      if (!b.w || !b.h) continue;
      const box = document.createElement('div');
      box.className = 'rst-box';
      box.style.left = pct(b.x, answer.width);
      box.style.top = pct(b.y, answer.height);
      box.style.width = pct(b.w, answer.width);
      box.style.height = pct(b.h, answer.height);
      box.dataset.radius = String(b.radius / answer.width);
      // Under lettering with no balloon round it, the art's own tone
      // carried in over the letters (a picture the app made).
      if (b.patch) {
        // Painted from its bytes, not loaded from a `data:` address: a
        // site's image policy may refuse those, and the lettering would show.
        const tone = document.createElement('canvas');
        tone.className = 'rst-patch';
        const [px, py, pw, ph] = b.patch.at;
        [tone.width, tone.height] = [pw, ph];
        tone.style.left = pct(px - b.x, b.w);
        tone.style.top = pct(py - b.y, b.h);
        tone.style.width = pct(pw, b.w);
        tone.style.height = pct(ph, b.h);
        const png = Uint8Array.from(atob(b.patch.png), (c) => c.charCodeAt(0));
        createImageBitmap(new Blob([png], { type: 'image/png' }))
          .then((bmp) => tone.getContext('2d').drawImage(bmp, 0, 0))
          .catch(() => {});
        box.appendChild(tone);
      }
      // The white follows the balloon's own outline where the app found it,
      // or is a rounded box where no letters were found; with a patch and no
      // outline there is none. The Korean is set above every cover, in a
      // white rim of its own.
      if (b.outline?.length || !b.patch) {
        const fill = document.createElement('div');
        fill.className = 'rst-fill';
        box.appendChild(fill);
        if (b.outline?.length) {
          fill.style.clipPath = `polygon(${b.outline.map(([x, y]) => `${pct(x - b.x, b.w)} ${pct(y - b.y, b.h)}`).join(',')})`;
        }
      }
      // A line lettered in several balloons is set a share in each.
      const parts = b.parts?.length ? b.parts : [{ text: b.text ?? [b.x, b.y, b.w, b.h], font_px: b.font_px, ko: b.ko }];
      for (const part of parts) {
        const text = document.createElement('div');
        text.className = 'rst-text';
        const [tx, ty, tw, th] = part.text;
        text.style.left = pct(tx - b.x, b.w);
        text.style.top = pct(ty - b.y, b.h);
        text.style.width = pct(tw, b.w);
        text.style.height = pct(th, b.h);
        text.dataset.font = String(part.font_px / answer.width);
        text.dataset.least = String((answer.lettering_px ?? 0) / answer.width);
        // A dash stays on the line of the word it follows: "아—?!" broke
        // after "아" (ch256 p7). A word joiner holds them together.
        text.textContent = part.ko.replace(/(\S)(?=[—–])/g, '$1⁠');
        // A line alone in its balloon runs along the balloon's outline:
        // short lines where it narrows, long ones through its middle, as the
        // original was lettered. A rectangle inside an oval leaves its
        // middle unused, and the Korean set small (ch258 p15).
        if (parts.length === 1 && b.outline?.length > 2) flowIn(text, part, b, pct, (b.flow_px ?? part.font_px) / answer.width);
        box.appendChild(text);
      }
      // A tap shows the original under it for a moment.
      box.onclick = () => box.classList.toggle('rst-peek');
      layer.appendChild(box);
      fitFont(box, img.offsetWidth);
    }
  }

  // The size the app chose from the original lettering, scaled to the
  // picture as shown, and no smaller than a phone reads without zooming
  // where the balloon has room. The browser breaks lines its own way, so
  // where its lines overflow the text area the size steps down: the shape
  // never grows past the balloon, and no line is cut off. Lines break
  // between words while the size keeps `WORDS_KEEP` of its own (and stays
  // readable); a balloon narrower than that has words broken between
  // syllables ("사과해야" / "겠구나") rather than the Korean set too small to
  // read, and only a word wider than the balloon anywhere.
  const READABLE = 7;
  const WORDS_KEEP = 0.7;
  function fitFont(box, shownWidth) {
    const fill = box.querySelector('.rst-fill');
    if (fill) fill.style.borderRadius = `${Number(box.dataset.radius) * shownWidth}px`;
    for (const text of box.querySelectorAll('.rst-text')) fitText(text, shownWidth);
  }

  // Where the outline is crossed at height `y`, the stretch of it round `x`
  // ([left, right] in page pixels), or null where `x` is not inside it.
  function spanAt(outline, y, x) {
    const xs = [];
    for (let i = 0; i < outline.length; i++) {
      const [x0, y0] = outline[i];
      const [x1, y1] = outline[(i + 1) % outline.length];
      if (y0 <= y !== y1 <= y) xs.push(x0 + ((y - y0) / (y1 - y0)) * (x1 - x0));
    }
    xs.sort((a, b) => a - b);
    for (let i = 0; i + 1 < xs.length; i += 2) if (xs[i] <= x && x <= xs[i + 1]) return [xs[i], xs[i + 1]];
    return null;
  }

  // Sets `text` across its balloon's width, centred on its text area, with
  // the outside of the outline taken up by two floats shaped to it: the
  // browser breaks each line where the balloon is as wide as it there.
  const BANDS = 48;
  function flowIn(text, part, b, pct, font) {
    const xs = b.outline.map((p) => p[0]);
    const ys = b.outline.map((p) => p[1]);
    const [ax, ar, oy0, oy1] = [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)];
    const [tx, ty, tw, th] = part.text;
    // Across, the middle of the balloon where the lettering's middle is:
    // a text area off to one side of it narrowed every line to its own
    // side's room (ch256 p16, "너무..." set below its middle).
    const cy = ty + th / 2;
    const row = spanAt(b.outline, cy, tx + tw / 2);
    const cx = row ? (row[0] + row[1]) / 2 : tx + tw / 2;
    const half = Math.min(cy - oy0, oy1 - cy);
    if (!(half > 0) || !(cx > ax && cx < ar)) return;
    const f = { outline: b.outline, x: ax, y: cy - half, w: ar - ax, h: 2 * half, cx };
    f.rect = [text.style.left, text.style.top, text.style.width, text.style.height, text.dataset.font];
    f.area = [pct(f.x - b.x, b.w), pct(f.y - b.y, b.h), pct(f.w, b.w), pct(f.h, b.h), String(font)];
    // Each band of rows keeps out what any of its rows has outside, as
    // much on both sides of the text's middle: the lines stay centred on
    // it, as lettering is (off it, a line beside a bulge of the outline
    // was set off to one side, ch256 p11).
    const [lw, rw] = [cx - ax, ar - cx];
    const left = ['0% 0%'];
    const right = ['100% 0%'];
    for (let k = 0; k < BANDS; k++) {
      let [l, r] = [ax, ar];
      for (let j = 0; j < 4; j++) {
        const s = spanAt(b.outline, f.y + ((k + (j + 0.5) / 4) / BANDS) * f.h, cx);
        [l, r] = s ? [Math.max(l, s[0]), Math.min(r, s[1])] : [cx, cx];
      }
      const half = Math.max(0, Math.min(cx - l, r - cx));
      [l, r] = [cx - half, cx + half];
      const [y0, y1] = [(k / BANDS) * 100, ((k + 1) / BANDS) * 100];
      const lx = (Math.min(Math.max(l - ax, 0), lw) / lw) * 100;
      const rx = (Math.min(Math.max(r - cx, 0), rw) / rw) * 100;
      left.push(`${lx}% ${y0}%`, `${lx}% ${y1}%`);
      right.push(`${rx}% ${y0}%`, `${rx}% ${y1}%`);
    }
    left.push('0% 100%');
    right.push('100% 100%');
    const side = (float, width, shape) => {
      const d = document.createElement('div');
      d.className = 'rst-side';
      d.style.cssText = `float:${float};width:${(width / f.w) * 100}%;height:100%;shape-outside:polygon(${shape.join(',')})`;
      return d;
    };
    // A short last word is not left alone on the last line ("보면 안 /
    // 돼?", ch256 p3): it goes down with the word before it, where the two
    // fit on a line.
    f.plain = text.textContent;
    f.tied = f.plain.replace(/ (\S{1,3})$/, '\u00a0$1');
    f.words = document.createElement('span');
    f.lift = document.createElement('div');
    f.nodes = [side('left', lw, left), side('right', rw, right), f.lift, f.words];
    text.rstShape = f;
  }

  // The Korean set along the outline (`on`), or in the lettering's own
  // area, centred.
  function flowing(text, on) {
    const f = text.rstShape;
    const [left, top, width, height, font] = on ? f.area : f.rect;
    Object.assign(text.style, { left, top, width, height, wordBreak: '', textWrap: '', overflowWrap: 'normal' });
    text.dataset.font = font;
    text.classList.toggle('rst-flow', on);
    text.rstFlow = on ? f : null;
    if (on) text.replaceChildren(...f.nodes);
    else text.textContent = f.plain;
  }

  // Lines set along the outline: centred in height, and each inside the
  // balloon where it is set (a word wider than the balloon there is set
  // below it, or past the outline).
  function flowOver(text, lines) {
    const f = text.rstFlow;
    const rects = () => {
      lines.selectNodeContents(f.words);
      return [...lines.getClientRects()].filter((r) => r.width > 0);
    };
    let lift = 0;
    f.lift.style.height = '0px';
    for (let k = 0; k < 4; k++) {
      const rs = rects();
      if (!rs.length) return false;
      // As much room above the lines as below: where the outline is too
      // narrow at the top for a line, the line is pushed down past the
      // spacer, and the spacer is cut by as much ("너무...", ch256 p16).
      const at = text.getBoundingClientRect();
      const want = Math.max(0, lift + (at.bottom - rs[rs.length - 1].bottom - (rs[0].top - at.top)) / 2);
      if (Math.abs(want - lift) < 1) break;
      lift = want;
      f.lift.style.height = `${lift}px`;
    }
    const a = text.getBoundingClientRect();
    // Pushed off the middle by a narrow outline above them, the lines are
    // set smaller rather than low in the balloon.
    const first = rects();
    const step = parseFloat(getComputedStyle(text).fontSize) * 1.2;
    if (Math.abs(a.bottom - first[first.length - 1].bottom - (first[0].top - a.top)) > step) return true;
    const k = a.width / f.w;
    if (!(k > 0)) return false;
    const tol = 1 / k;
    // A band too narrow for a line leaves a gap among them (ch256 p12,
    // "아... 아버 / / 지...!"), and a quarter line already shows (p11, "이건
    // 허위 / / 광고야!" in the narrow half of a joined balloon): set
    // smaller, the lines run on.
    const rs = rects();
    const tops = [...new Set(rs.map((r) => Math.round(r.top)))].sort((p, q) => p - q);
    if (tops.some((t, i) => i > 0 && t - tops[i - 1] > step * 1.25)) return true;
    return rs.some((r) => {
      const [y0, y1] = [f.y + (r.top - a.top) / k, f.y + (r.bottom - a.top) / k];
      const [x0, x1] = [f.x + (r.left - a.left) / k, f.x + (r.right - a.left) / k];
      if (y0 < f.y - tol || y1 > f.y + f.h + tol) return true;
      return [y0 + tol, (y0 + y1) / 2, y1 - tol].some((y) => {
        const s = spanAt(f.outline, y, Math.min(Math.max((x0 + x1) / 2, f.x), f.x + f.w));
        return !s || x0 < s[0] - tol || x1 > s[1] + tol;
      });
    });
  }

  // Lines set along the outline are filled in turn: the browser balances
  // no lines beside floats. Both sides are drawn in as far as the lines
  // stay as many, as tall, and inside the balloon, as balancing would.
  function squeeze(text, px) {
    for (const side of text.querySelectorAll('.rst-side')) side.style.shapeMargin = px ? `calc(min(0.5em, 6%) + ${px}px)` : '';
  }

  function balance(text, lines) {
    const f = text.rstFlow;
    // How many lines, and how tall: a band too narrow for any line opens
    // a gap among them.
    const shape = () => {
      lines.selectNodeContents(f.words);
      const rs = [...lines.getClientRects()].filter((r) => r.width > 0);
      return rs.length ? [new Set(rs.map((r) => Math.round(r.top))).size, rs[rs.length - 1].bottom - rs[0].top] : [0, 0];
    };
    const [n, tall] = shape();
    let [lo, hi] = [0, text.clientWidth / 2];
    for (let k = 0; k < 6; k++) {
      const mid = (lo + hi) / 2;
      squeeze(text, mid);
      const over = flowOver(text, lines);
      const [m, t] = shape();
      if (!over && m === n && t <= tall + 1) lo = mid;
      else hi = mid;
    }
    squeeze(text, lo);
    flowOver(text, lines);
  }

  // Set along the outline only where it reads larger than in the
  // lettering's own area: a notch in a caption's outline left a gap among
  // the lines, and them small (ch256 p17, "난 그래도 네가 좋아!"); a word
  // too long for an oval's narrow rows was broken a syllable a line
  // ("이 / 히 / 히...", ch256 p8).
  // Words kept whole come first, then the size: "아빠 / 가", "돌려 /
  // 주마!" set larger along the outline read worse than whole words in
  // the area (ch256 p5).
  function fitText(text, shownWidth) {
    if (!text.rstShape) return fitIn(text, shownWidth);
    flowing(text, true);
    const flowed = fitIn(text, shownWidth);
    const along = parseFloat(text.style.fontSize);
    flowing(text, false);
    const plain = fitIn(text, shownWidth);
    if (flowed > plain || (flowed === plain && flowed && along > parseFloat(text.style.fontSize) * 1.03)) {
      flowing(text, true);
      fitIn(text, shownWidth);
    }
  }

  // Fits the Korean at the largest size it takes: 2 with its words whole,
  // 1 broken between syllables (in the area only), 0 where it does not
  // fit (along the outline; in the area it is set anyway).
  function fitIn(text, shownWidth) {
    const start = Math.max(READABLE, Number(text.dataset.font) * shownWidth);
    // A centred line wider than the text area spills past both sides, and
    // past the left the browser does not count as scrolled: the lines'
    // own extent is compared with the area too (ch257 p19, "젠장...!").
    const lines = document.createRange();
    const spills = () => {
      lines.selectNodeContents(text);
      const [l, a] = [lines.getBoundingClientRect(), text.getBoundingClientRect()];
      return l.width > 0 && (l.left < a.left - 1 || l.right > a.right + 1 || l.top < a.top - 1 || l.bottom > a.bottom + 1);
    };
    // A syllable alone on a line, cut from its word ("이 / 히 / 히...",
    // ch256 p8): set smaller, its word keeps two a line.
    const lonely = () => {
      lines.selectNodeContents(text.rstFlow?.words ?? text);
      const rs = [...lines.getClientRects()].filter((r) => r.width > 0);
      return rs.length > 1 && rs.some((r) => r.width < 1.3 * parseFloat(text.style.fontSize));
    };
    const shrink = (floor, alone) => {
      let px = start;
      for (;;) {
        text.style.fontSize = `${px}px`;
        const over = (text.rstFlow
          ? flowOver(text, lines)
          : text.scrollHeight > text.clientHeight + 1 || text.scrollWidth > text.clientWidth + 1 || spills()) || (alone && lonely());
        if (!over || px <= floor) return over;
        px = Math.max(floor, px * 0.92);
      }
    };
    text.style.wordBreak = '';
    text.style.textWrap = '';
    text.style.overflowWrap = 'normal';
    text.style.overflow = '';
    const f = text.rstFlow;
    if (f) {
      squeeze(text, 0);
      f.words.textContent = f.tied;
    }
    let kept = !shrink(Math.max(READABLE, start * WORDS_KEEP));
    if (!kept && f && f.tied !== f.plain) {
      f.words.textContent = f.plain;
      kept = !shrink(Math.max(READABLE, start * WORDS_KEEP));
    }
    if (f && kept) balance(text, lines);
    if (kept) return 2;
    // Along the outline the words stay whole: broken there, "걸 / 로 잡 /
    // 아줄 / 게—!" (ch257 p17).
    if (f) return 0;
    // Between syllables, as Korean may break, but never ahead of the
    // punctuation that ends a word: "무서워하지 않았어...?" keeps its
    // "...?", "젠장...!" its "!" (ch257 p18, p19). Lines filled in turn,
    // not balanced: balanced, a short word is split too ("너 / 무 달",
    // ch257 p6).
    text.style.wordBreak = 'normal';
    text.style.textWrap = 'wrap';
    if (!shrink(READABLE, true) || !shrink(READABLE)) return 1;
    // The least size: readable, or, where the page is shown so small that
    // its own lettering is smaller (a two-page spread across a phone, ch258
    // p18), as large as the original lettering there, never below it.
    text.style.overflowWrap = 'anywhere';
    if (!shrink(Math.min(READABLE, Number(text.dataset.least) * shownWidth || READABLE))) return 1;
    // Over even at the floor: the floor holds (scaled below it, "이거였어!"
    // on the ch258 p18 spread read smaller than its own lettering). The
    // lines stay whole at that size, centred on the text area and shown
    // past its edge, over the cover round it, in their white rim.
    text.style.overflow = 'visible';
    return 0;
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
