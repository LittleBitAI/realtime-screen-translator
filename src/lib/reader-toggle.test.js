// The reader's page script (src-tauri/src/reader.js) on a chapter page,
// switched to the original and back. Apart from reader.test.js: a reader
// started there keeps watching the page and would take this one's picture.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import script from '../../src-tauri/src/reader.js?raw';

const page = () => ({
  width: 600,
  height: 800,
  saved: false,
  boxes: [{ x: 60, y: 80, w: 120, h: 60, radius: 6, font_px: 20, ko: '안녕', outline: [], text: [66, 86, 108, 48] }],
  save_error: null,
});

beforeEach(() => {
  vi.stubGlobal('setInterval', vi.fn());
  vi.stubGlobal('requestAnimationFrame', (f) => setTimeout(f, 0));
  Range.prototype.getBoundingClientRect ??= () => new DOMRect();
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('reader page script, original and back', () => {
  // ch256 p15 at S21 size: placed while the original was shown, the Korean
  // came back three pages down, over p18 and p19, and p15 was bare.
  it('places a page’s Korean over it when the translation comes back on', async () => {
    history.replaceState(null, '', '/chapter.html?id=4');
    document.head.innerHTML = '<meta name="rst-app"><meta name="rst-chapter">';
    document.body.innerHTML = '<h1></h1><main></main>';
    vi.stubGlobal('URL', class extends URL {
      static createObjectURL() { return 'blob:fixture-page'; }
      static revokeObjectURL() {}
    });
    // The page scrolled 500 px: the layer's box starts that far above the view.
    let top = 300;
    vi.spyOn(HTMLImageElement.prototype, 'naturalWidth', 'get').mockReturnValue(600);
    vi.spyOn(HTMLImageElement.prototype, 'complete', 'get').mockReturnValue(true);
    vi.spyOn(HTMLElement.prototype, 'offsetTop', 'get').mockImplementation(function () {
      return this.tagName === 'IMG' ? top + 500 : 0;
    });
    vi.spyOn(Element.prototype, 'getBoundingClientRect').mockImplementation(function () {
      if (this.tagName === 'IMG') return { top, bottom: top + 800, left: 0, right: 600, width: 600, height: 800 };
      if (!this.classList.contains('rst-layer') || document.documentElement.classList.contains('rst-off')) return new DOMRect();
      return new DOMRect(parseFloat(this.style.left) || 0, -500 + (parseFloat(this.style.top) || 0), parseFloat(this.style.width) || 0, 0);
    });
    const invoke = vi.fn().mockImplementation(async (command) => ({
      get_chapter: { title: 'Fixture', pages: [{ id: 17, width: 600, height: 800 }] },
      get_chapter_image: new Uint8Array([1, 2, 3]).buffer,
      post_chapter_page: page(),
    })[command]);
    window.__TAURI_INTERNALS__ = { invoke };
    new Function(script)();

    const layer = await vi.waitFor(() => {
      const l = document.querySelector('.rst-layer');
      expect(l?.querySelector('.rst-box')).toBeTruthy();
      return l;
    });
    expect(layer.style.top).toBe('800px');
    const toggle = [...document.querySelectorAll('.rst-bar button')].find((b) => b.textContent === '번역 켜짐');
    toggle.click();
    // The original shown, pages above it settle and it moves down.
    top = 400;
    window.dispatchEvent(new Event('scroll'));
    await new Promise((r) => setTimeout(r, 10));
    toggle.click();
    expect(layer.style.top).toBe('900px');
  });
});
