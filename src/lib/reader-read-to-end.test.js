import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import script from '../../src-tauri/src/reader.js?raw';
import { blank } from './reader-chapter-fixture.js';

const button = () => [...document.querySelectorAll('.rst-bar button')].find((b) => b.textContent === '끝까지 번역');
const status = () => document.querySelector('.rst-bar [role=status]').textContent;
const addresses = (n) => Array.from({ length: n }, (_, i) => `https://pictures.example/${i}.jpg`);
const answer = { ...blank, boxes: [{ x: 40, y: 60, w: 120, h: 60, radius: 5, font_px: 14, ko: '안녕' }] };
const listURL = 'https://manga.example/chapter-index';
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

let observe;
let listeners;
let frames;

beforeEach(() => {
  observe = vi.spyOn(MutationObserver.prototype, 'observe');
  listeners = vi.spyOn(window, 'addEventListener');
  frames = [];
  vi.stubGlobal('setInterval', vi.fn());
  vi.stubGlobal('requestAnimationFrame', (fn) => frames.push(setTimeout(fn, 0)));
  Range.prototype.getBoundingClientRect ??= () => new DOMRect();
  Range.prototype.getClientRects ??= () => [];
});

afterEach(() => {
  for (const observer of observe.mock.instances) observer.disconnect();
  for (const args of listeners.mock.calls) window.removeEventListener(...args);
  for (const timer of frames) clearTimeout(timer);
  document.head.replaceChildren();
  document.body.replaceChildren();
  history.replaceState(null, '', '/');
  delete window.__rstReader;
  delete window.__TAURI_INTERNALS__;
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

function fixture(urls, { visible = 0, mounted = urls.map((_, i) => i), siteResponses = new Map(), reply = async () => answer } = {}) {
  history.replaceState(null, '', '/chapter/21');
  document.title = 'Test chapter';
  let view = visible;
  const slides = Array.from({ length: urls.length + 1 }, () => {
    const slide = document.createElement('div');
    slide.className = 'swiper-slide';
    document.body.append(slide);
    return slide;
  });
  function mount(index, { loaded = true, src = urls[index], lazy } = {}) {
    const img = document.createElement('img');
    if (src) img.setAttribute('src', src);
    if (lazy) img.setAttribute('data-src', lazy);
    const load = (address = src) => {
      if (address) img.setAttribute('src', address);
      loaded = true;
    };
    const props = { complete: () => loaded, naturalWidth: () => loaded ? 600 : 0, naturalHeight: () => loaded ? 800 : 0,
      offsetWidth: () => 300, offsetHeight: () => 400, currentSrc: () => loaded ? img.src : '' };
    for (const [name, get] of Object.entries(props)) Object.defineProperty(img, name, { get });
    img.getBoundingClientRect = () => new DOMRect((index - view) * innerWidth * 3, 0, 300, 400);
    slides[index].append(img);
    return { img, load };
  }
  for (const index of mounted) mount(index);

  class ReaderXHR extends EventTarget {
    responseURL = listURL;
    responseType = '';
    responseText = '';
    contentType = 'application/json';
    getResponseHeader() { return this.contentType; }
    send() { this.dispatchEvent(new Event('load')); return 'native send result'; }
  }
  vi.stubGlobal('XMLHttpRequest', ReaderXHR);
  const fetch = vi.fn(async (url) => {
    if (siteResponses.has(url)) return siteResponses.get(url);
    if (url === urls[urls.length - 1]) return { ok: true, blob: async () => new Blob(['sample'], { type: 'image/jpeg' }) };
    throw new TypeError('cors');
  });
  vi.stubGlobal('fetch', fetch);
  const invoke = vi.fn((command, args) => reply(args, command));
  window.__TAURI_INTERNALS__ = { invoke };
  new Function(script)();

  function xhr(value, options = {}) {
    const request = Object.assign(new XMLHttpRequest(), { responseText: JSON.stringify(value) }, options);
    const load = vi.fn();
    request.addEventListener('load', load);
    expect(request.send()).toBe('native send result');
    expect(load).toHaveBeenCalledOnce();
    return request;
  }
  const calls = () => invoke.mock.calls.map(([command, args]) => {
    expect(command).toBe('post_reader_page');
    return args;
  });
  return { calls, indices: () => calls().map((args) => args.index), mount, slides, fetch, xhr, view: (index) => { view = index; } };
}

function response(body, url = listURL, contentType = 'application/json') {
  const text = vi.fn(async () => body);
  return { url, headers: { get: () => contentType }, clone: vi.fn(() => ({ text })) };
}

it('uses the matching XHR list of varying-size entries, stays in place and never overlaps calls', async () => {
  const urls = addresses(6);
  const release = [];
  const site = fixture(urls, { visible: 2, mounted: [2, 3], reply: () => new Promise((ok) => release.push(ok)) });
  const list = { data: { pages: urls.map((url, i) => ({ url, width: i % 2 ? 2230 : 1115, height: 1600 })) } };
  const xhr = site.xhr(list);
  expect(JSON.parse(xhr.responseText)).toEqual(list);
  site.xhr({ data: { pages: [urls[2], urls[4], urls[3], urls[5]] } });
  site.xhr({ data: { pages: [urls[0], urls[1], urls[3], urls[5]] } });
  expect(button().getAttribute('aria-pressed')).toBe('false');
  expect(document.head.textContent).toContain('.rst-bar button{min-height:48px');
  const scrollTo = vi.spyOn(window, 'scrollTo').mockImplementation(() => {});
  const scrollBy = vi.spyOn(window, 'scrollBy').mockImplementation(() => {});
  const readerAddress = location.href;
  const original = [...document.images].map((img) => img.outerHTML);
  await vi.waitFor(() => expect(site.indices()).toEqual([2]));
  await settle();
  expect(site.indices()).toEqual([2]);
  button().click();
  expect(button().getAttribute('aria-pressed')).toBe('true');
  expect(status()).toContain('남은 페이지 4장');
  expect(site.indices()).toEqual([2]);
  for (let index = 2; index <= 5; index++) {
    await vi.waitFor(() => expect(site.indices()).toEqual(urls.slice(2, index + 1).map((_, i) => i + 2)));
    expect(release).toHaveLength(1);
    release.shift()(answer);
  }
  await vi.waitFor(() => expect(status()).toBe('회차 끝까지 번역했습니다.'));
  expect(button().getAttribute('aria-pressed')).toBe('false');
  expect(site.calls()).toEqual(urls.slice(2).map((src, i) => ({ src, image: i === 3 ? 'c2FtcGxl' : null,
    page_url: location.href, title: 'Test chapter', index: i + 2 })));
  expect(site.slides.map((slide) => Boolean(slide.firstElementChild))).toEqual([false, false, true, true, false, false, false]);
  expect([...document.images].map((img) => img.outerHTML)).toEqual(original);
  expect(site.fetch.mock.calls.flat()).toEqual(urls.slice(2));
  expect(scrollTo).not.toHaveBeenCalled();
  expect(scrollBy).not.toHaveBeenCalled();
  expect(location.href).toBe(readerAddress);

  site.mount(5);
  await vi.waitFor(() => expect(site.slides[5].querySelector('.rst-text')?.textContent).toBe('안녕'));
  expect(site.indices()).toEqual([2, 3, 4, 5]);
});

it.each(['fetch', 'XHR text', 'XHR json'])('passively accepts ordered addresses from %s before DOMContentLoaded', async (transport) => {
  const urls = addresses(3);
  const wrapped = response(JSON.stringify({ nested: [urls] }));
  const ready = vi.spyOn(document, 'readyState', 'get').mockReturnValue('loading');
  const site = fixture(urls, { mounted: [0], siteResponses: new Map([[listURL, wrapped]]) });
  if (transport === 'fetch') {
    expect(await window.fetch(listURL)).toBe(wrapped);
    await settle();
    expect(wrapped.clone).toHaveBeenCalledOnce();
  } else if (transport === 'XHR text') site.xhr({ nested: urls.map((url) => [url, 1, 0]) }, { contentType: 'text/plain' });
  else site.xhr(null, { responseType: 'json', response: { nested: urls } });
  expect(site.indices()).toEqual([]);
  ready.mockRestore();
  document.dispatchEvent(new Event('DOMContentLoaded'));
  await vi.waitFor(() => expect(site.indices()).toEqual([0]));
  button().click();
  await vi.waitFor(() => expect(status()).toBe('회차 끝까지 번역했습니다.'));
  expect(site.indices()).toEqual([0, 1, 2]);
});

it('ignores an invalid body, localhost, the wrong media type, and picture fetches', async () => {
  const urls = addresses(3);
  const invalid = response('not JSON');
  const local = response(JSON.stringify(urls), 'http://ipc.localhost/chapter');
  const media = response(JSON.stringify(urls), listURL, 'image/jpeg');
  const picture = { ...response(JSON.stringify(urls)), ok: true, blob: async () => new Blob(['sample'], { type: 'image/jpeg' }) };
  const site = fixture(urls, { mounted: [0, 1], siteResponses: new Map([
    ['bad', invalid], ['ipc', local], ['image', media], [urls[0], picture],
  ]) });
  await Promise.all(['bad', 'ipc', 'image'].map((key) => window.fetch(key)));
  site.xhr(urls, { responseType: 'arraybuffer' });
  await vi.waitFor(() => expect(site.indices()).toEqual([0]));
  button().click();
  await vi.waitFor(() => expect(status()).toBe('사이트가 불러온 페이지를 번역했습니다.'));
  expect(site.indices()).toEqual([0, 1]);
  expect(invalid.clone).toHaveBeenCalledOnce();
  for (const other of [local, media, picture]) expect(other.clone).not.toHaveBeenCalled();
});

it('drops old lists after twenty newer lists and skips a body over the size limit', async () => {
  const urls = addresses(3);
  const large = response(' '.repeat(2_000_000) + JSON.stringify(urls));
  const site = fixture(urls, { mounted: [0], siteResponses: new Map([[listURL, large]]) });
  site.xhr(urls);
  for (let index = 0; index < 20; index++) site.xhr([`https://other.example/${index}`, 'https://other.example/end']);
  await window.fetch(listURL);
  await settle();
  await vi.waitFor(() => expect(site.indices()).toEqual([0]));
  button().click();
  await vi.waitFor(() => expect(status()).toBe('사이트가 불러온 페이지를 번역했습니다.'));
  expect(site.indices()).toEqual([0]);
});

it('reads loaded and lazy pictures by their own address, with no request for an addressless slot', async () => {
  const urls = addresses(6);
  const site = fixture(urls, { visible: 1, mounted: [0, 1] });
  const lazy = site.mount(2, { loaded: false, src: '', lazy: urls[2] });
  site.mount(3, { loaded: false, src: '' });
  site.mount(4);
  site.mount(5, { loaded: false });
  await vi.waitFor(() => expect(site.indices()).toEqual([1]));
  button().click();
  await vi.waitFor(() => expect(status()).toBe('번역하지 못한 페이지 1장'));
  expect(site.indices()).toEqual([1, 2, 4, 5]);
  expect(site.calls().map(({ image }) => image)).toEqual([null, null, null, 'c2FtcGxl']);
  expect(document.querySelector('.rst-chip')).toBeNull();
  expect(lazy.img.getAttribute('src')).toBeNull();
  lazy.load(urls[2]);
  lazy.img.dispatchEvent(new Event('load'));
  const scan = setInterval.mock.calls[0][0];
  scan();
  await vi.waitFor(() => expect(site.slides[2].querySelector('.rst-text')?.textContent).toBe('안녕'));
  expect(site.indices()).toEqual([1, 2, 4, 5]);
});

it('continues past skippable failures and reads a skipped list page when the site mounts it', async () => {
  const urls = addresses(6);
  const codes = ['image_unreachable', 'bad_image', 'image_dimensions_too_large', 'gemini_unreadable'];
  let recover = false;
  const site = fixture(urls, { mounted: [0, 2], reply: async ({ index }) => {
    if (codes[index - 1] && !recover) throw codes[index - 1];
    return answer;
  } });
  site.xhr(urls);
  await vi.waitFor(() => expect(site.indices()).toEqual([0]));
  button().click();
  await vi.waitFor(() => expect(status()).toBe('번역하지 못한 페이지 4장'));
  expect(site.indices()).toEqual([0, 1, 2, 3, 4, 4, 5]);
  expect(site.slides[1].children).toHaveLength(0);
  expect(site.slides[2].querySelector('.rst-chip')?.textContent).toContain('번역 실패');

  recover = true;
  site.view(1);
  site.mount(1);
  await vi.waitFor(() => expect(site.slides[1].querySelector('.rst-text')?.textContent).toBe('안녕'));
  expect(site.indices()).toEqual([0, 1, 2, 3, 4, 4, 5, 1]);
  button().click();
  await vi.waitFor(() => expect(status()).toBe('회차 끝까지 번역했습니다.'));
  expect(site.indices()).toEqual([0, 1, 2, 3, 4, 4, 5, 1, 2, 3, 4]);
});

it.each([
  ['key_missing', 1, '키를 넣어'], ['key_invalid', 1, '키가 맞지'], ['gemini_quota', 1, '한도'],
  ['gemini_busy', 2, '혼잡'], ['gemini_unreachable', 2, '인터넷 연결'], ['gemini_refused', 1, '거절'],
  ['gemini_refused_400', 1, 'HTTP 400'], ['gemini_refused_404', 1, 'HTTP 404'],
  ['gemini_dev_unreachable', 1, '개발용'], [new Error('IPC'), 1, '앱에서 번역을 요청하지'],
  ['storage_full', 1, '저장 공간'],
])('stops with its own status after %s', async (code, attempts, message) => {
  const site = fixture(addresses(3), { reply: async ({ index }) => {
    if (index !== 1) return answer;
    if (code === 'storage_full') return { ...answer, save_error: code };
    throw code;
  } });
  await vi.waitFor(() => expect(site.indices()).toEqual([0]));
  button().click();
  await vi.waitFor(() => expect(status()).toContain(message));
  expect(site.indices()).toEqual([0, ...Array(attempts).fill(1)]);
  expect(button().getAttribute('aria-pressed')).toBe('false');
  expect(site.slides[1].querySelector('.rst-chip')).not.toBeNull();
});

it.each(['끝까지 번역', '번역 켜짐'])('cancels after the in-flight call when %s is pressed', async (label) => {
  let finish;
  const site = fixture(addresses(3), { reply: async ({ index }) => index === 1 ? new Promise((ok) => { finish = ok; }) : answer });
  await vi.waitFor(() => expect(site.indices()).toEqual([0]));
  button().click();
  await vi.waitFor(() => expect(site.indices()).toEqual([0, 1]));
  [...document.querySelectorAll('.rst-bar button')].find((b) => b.textContent === label).click();
  expect(button().getAttribute('aria-pressed')).toBe('false');
  finish(answer);
  await settle();
  expect(site.indices()).toEqual([0, 1]);
  expect(site.slides[1].querySelector('.rst-chip')).toBeNull();
});

it('restarts from the newly visible page while a canceled request settles', async () => {
  let finish;
  const site = fixture(addresses(4), { reply: async ({ index }) => index === 1 ? new Promise((ok) => { finish = ok; }) : answer });
  await vi.waitFor(() => expect(site.indices()).toEqual([0]));
  button().click();
  await vi.waitFor(() => expect(site.indices()).toEqual([0, 1]));
  button().click();
  site.view(2);
  button().click();
  expect(site.indices()).toEqual([0, 1]);
  finish(answer);
  await vi.waitFor(() => expect(status()).toBe('사이트가 불러온 페이지를 번역했습니다.'));
  expect(site.indices()).toEqual([0, 1, 2, 3]);
});

it('uses the failure of the visible in-flight page when a run takes it over', async () => {
  let fail;
  const site = fixture(addresses(3), { reply: ({ index }) => index === 0 ? new Promise((_, reject) => { fail = reject; }) : answer });
  await vi.waitFor(() => expect(site.indices()).toEqual([0]));
  button().click();
  fail('gemini_quota');
  await vi.waitFor(() => expect(status()).toContain('한도'));
  expect(button().getAttribute('aria-pressed')).toBe('false');
  expect(site.indices()).toEqual([0]);
});

it('keeps the button unpressed when there is no visible picture', async () => {
  const site = fixture(addresses(2), { visible: -1 });
  button().click();
  await settle();
  expect(button().getAttribute('aria-pressed')).toBe('false');
  expect(site.indices()).toEqual([]);
});

it('omits the button from the imported-chapter toolbar', async () => {
  document.head.innerHTML = '<meta name="rst-app"><meta name="rst-chapter">';
  document.body.innerHTML = '<h1></h1><main></main>';
  window.__TAURI_INTERNALS__ = { invoke: vi.fn() };
  new Function(script)();
  await vi.waitFor(() => expect(document.querySelectorAll('.rst-bar button')).toHaveLength(2));
  expect(button()).toBeUndefined();
});
