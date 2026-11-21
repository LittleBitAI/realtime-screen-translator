// An imported chapter opened in the reader's page script
// (src-tauri/src/reader.js) under jsdom, for the chapter tests. One chapter
// per test file: a script started keeps watching the document.
import { vi } from 'vitest';
import script from '../../src-tauri/src/reader.js?raw';

/**
 * Opens a chapter of `ids` pages. `image(id)` answers `get_chapter_image`,
 * `page(id)` answers `post_chapter_page`, `top(id)` places each picture
 * (in view pixels), and pictures listed in `broken` fail to decode.
 */
export function openChapter({ ids, image = async () => new Uint8Array([1, 2, 3]).buffer, page, top = () => 0, broken = [] }) {
  history.replaceState(null, '', '/chapter.html?id=4');
  document.head.innerHTML = '<meta name="rst-app"><meta name="rst-chapter">';
  document.body.innerHTML = '<h1></h1><main></main>';
  let made = 0;
  vi.stubGlobal('URL', class extends URL {
    static createObjectURL() { made += 1; return `blob:fixture-page-${made}`; }
    static revokeObjectURL() {}
  });
  const id = (img) => Number(img.dataset.pictureId);
  vi.spyOn(HTMLImageElement.prototype, 'naturalWidth', 'get').mockImplementation(function () {
    return this.hasAttribute('src') && !broken.includes(id(this)) ? 600 : 0;
  });
  vi.spyOn(HTMLImageElement.prototype, 'complete', 'get').mockReturnValue(true);
  vi.spyOn(Element.prototype, 'getBoundingClientRect').mockImplementation(function () {
    if (this.tagName !== 'IMG') return new DOMRect();
    const t = top(id(this));
    return { top: t, bottom: t + 400, left: 0, right: 300, width: 300, height: 400 };
  });
  const invoke = vi.fn().mockImplementation(async (command, args) => {
    if (command === 'get_chapter') return { title: 'Fixture', pages: ids.map((i) => ({ id: i, width: 600, height: 800 })) };
    if (command === 'get_chapter_image') return image(args.picture_id);
    if (command === 'post_chapter_page') return page(args.picture_id);
    return undefined;
  });
  window.__TAURI_INTERNALS__ = { invoke };
  // A picture given its bytes loads, or fails to decode, as in a browser.
  new MutationObserver((changes) => changes.forEach((c) => {
    if (c.target.hasAttribute('src')) c.target.dispatchEvent(new Event(broken.includes(id(c.target)) ? 'error' : 'load'));
  })).observe(document.body, { subtree: true, attributeFilter: ['src'] });
  new Function(script)();
  const sent = () => invoke.mock.calls.filter(([c]) => c === 'post_chapter_page').map(([, a]) => a.picture_id);
  const toggle = () => [...document.querySelectorAll('.rst-bar button')].find((b) => b.hasAttribute('aria-pressed'));
  return { invoke, sent, toggle };
}

export const blank = { width: 600, height: 800, saved: false, boxes: [], save_error: null };
