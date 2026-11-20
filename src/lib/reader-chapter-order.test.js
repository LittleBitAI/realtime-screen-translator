// The reader's page script (src-tauri/src/reader.js) sending an imported
// chapter's pages to the app. Apart from the other script tests: a reader
// started there keeps watching the document and would send these pages too.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import script from '../../src-tauri/src/reader.js?raw';

beforeEach(() => {
  vi.stubGlobal('setInterval', vi.fn());
  vi.stubGlobal('requestAnimationFrame', (f) => setTimeout(f, 0));
  Range.prototype.getBoundingClientRect ??= () => new DOMRect();
  Range.prototype.getClientRects ??= () => [];
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('reader page script, chapter pages in order', () => {
  // An imported chapter: the app can only have page 2 wait for page 1 once
  // page 1 is sent, so page 2 is not sent while page 1's picture loads.
  it("sends chapter pages in reading order even when a later page's picture loads sooner", async () => {
    history.replaceState(null, '', '/chapter.html?id=4');
    document.head.innerHTML = '<meta name="rst-app"><meta name="rst-chapter">';
    document.body.innerHTML = '<h1></h1><main></main>';
    let made = 0;
    vi.stubGlobal('URL', class extends URL {
      static createObjectURL() { made += 1; return `blob:fixture-page-${made}`; }
      static revokeObjectURL() {}
    });
    vi.spyOn(HTMLImageElement.prototype, 'naturalWidth', 'get').mockReturnValue(600);
    vi.spyOn(HTMLImageElement.prototype, 'complete', 'get').mockReturnValue(true);
    vi.spyOn(Element.prototype, 'getBoundingClientRect').mockImplementation(function () {
      return this.tagName === 'IMG' ? { top: 0, bottom: 400, left: 0, right: 300, width: 300, height: 400 } : new DOMRect();
    });
    const answer = { width: 600, height: 800, saved: false, boxes: [], save_error: null };
    const invoke = vi.fn().mockImplementation(async (command, args) => {
      if (command === 'get_chapter') return { title: 'Fixture', pages: [{ id: 11, width: 600, height: 800 }, { id: 12, width: 600, height: 800 }] };
      if (command === 'get_chapter_image') {
        if (args.picture_id === 11) await new Promise((ok) => setTimeout(ok, 50));
        return new Uint8Array([1, 2, 3]).buffer;
      }
      if (command === 'post_chapter_page') return answer;
      return undefined;
    });
    window.__TAURI_INTERNALS__ = { invoke };
    // A picture given its bytes loads, as in a browser.
    new MutationObserver((changes) => changes.forEach((c) => c.target.hasAttribute('src') && c.target.dispatchEvent(new Event('load'))))
      .observe(document.body, { subtree: true, attributeFilter: ['src'] });

    new Function(script)();

    const sent = () => invoke.mock.calls.filter(([c]) => c === 'post_chapter_page').map(([, a]) => a.picture_id);
    await vi.waitFor(() => expect(sent()).toHaveLength(2), { timeout: 2000 });
    expect(sent()).toEqual([11, 12]);
  });
});
