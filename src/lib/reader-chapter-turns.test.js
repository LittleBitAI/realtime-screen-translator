// The reader's page script (src-tauri/src/reader.js) sending an imported
// chapter's pages to the app. One chapter per file: a script started keeps
// watching the document.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { blank, openChapter } from './reader-chapter-fixture.js';

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

describe('reader page script, one chapter page at a time', () => {
  // A page's request carries the lines of the earlier pages the app has
  // kept: each page is sent only once the one before it has been answered,
  // whichever page is nearest the view when translation comes back on.
  it('sends each chapter page only after the earlier page has been answered', async () => {
    const events = [];
    const replies = new Map();
    const h = innerHeight;
    // Page 13 in view, 12 below it, 11 farthest; all in reach.
    const { sent, toggle } = openChapter({
      ids: [11, 12, 13],
      top: (id) => ({ 11: 2.4 * h, 12: 1.7 * h, 13: 0 })[id],
      page: (id) => {
        events.push(`sent ${id}`);
        return new Promise((ok) => replies.set(id, () => { events.push(`answered ${id}`); ok(blank); }));
      },
    });
    toggle().click();
    // Every picture loaded while translation was off.
    await vi.waitFor(() => expect(document.querySelectorAll('main img[src]')).toHaveLength(3));
    await new Promise((ok) => setTimeout(ok, 20));
    expect(sent()).toEqual([]);
    toggle().click();
    for (const id of [11, 12, 13]) {
      await vi.waitFor(() => expect(replies.has(id)).toBe(true));
      await new Promise((ok) => setTimeout(ok, 20));
      replies.get(id)();
    }
    await vi.waitFor(() => expect(events).toHaveLength(6));
    expect(events).toEqual(['sent 11', 'answered 11', 'sent 12', 'answered 12', 'sent 13', 'answered 13']);
  });
});
