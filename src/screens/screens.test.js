import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { api, ApiError } from '../lib/api.js';
import Home from './Home.svelte';
import Library from './Library.svelte';
import Options from './Options.svelte';

vi.mock('../lib/api.js', async (real) => {
  const mod = await real();
  return {
    ...mod,
    api: {
      status: vi.fn(),
      putKey: vi.fn(),
      putSourceLang: vi.fn(),
      canOpenSites: vi.fn(() => true),
      openSite: vi.fn(),
      books: vi.fn(),
      lists: vi.fn(async () => []),
      exportBook: vi.fn(),
      previewSrc: vi.fn(async (p) => `/files/${p}`),
      // Inside the app by default: exports are already in the gallery.
      exportLinks: vi.fn(() => []),
    },
  };
});

const off = {
  key_set: false,
  key_tail: null,
  notice: null,
  gemini_requests: 0,
  source_lang: 'ja',
};
const never = () => new Promise(() => {});
const failing = async () => {
  throw new ApiError('internal', 500);
};
const feature = (id) => document.querySelectorAll(`[data-feature="${id}"]`);

beforeEach(() => {
  vi.clearAllMocks();
  api.canOpenSites.mockReturnValue(true);
  localStorage.clear();
});
afterEach(cleanup);

describe('S1 Home', () => {
  const typeUrl = (value) => fireEvent.input(screen.getByLabelText('주소'), { target: { value } });
  const openBar = () => feature('open-site')[0];

  it('loading: grey bars, and nothing to open before an address', () => {
    api.status.mockImplementation(never);
    render(Home);
    expect(openBar().disabled).toBe(true);
    expect(screen.getByText('상태를 읽는 중입니다.')).toBeTruthy();
  });

  it('empty: no key, asks for the key first and opens nothing', async () => {
    api.status.mockResolvedValue(off);
    await act(() => render(Home));
    expect(screen.getByText('번역하려면 Gemini API 키를 먼저 넣어 주세요.')).toBeTruthy();
    expect(screen.getByText('없음')).toBeTruthy();
    const optionsLink = screen.getByText('설정으로 가기', { selector: 'a[href]' });
    expect(optionsLink.getAttribute('href')).toBe('/options');
    await typeUrl('https://example.com/manga/1');
    await fireEvent.click(openBar());
    expect(screen.getByText(/키가 없습니다/)).toBeTruthy();
    expect(api.openSite).not.toHaveBeenCalled();
  });

  it('error: a 500 says so and offers to read again', async () => {
    api.status.mockImplementation(failing);
    render(Home);
    await screen.findByText(/상태를 읽지 못했습니다/);
    expect(screen.getByRole('button', { name: '다시 읽기' })).toBeTruthy();
    // Every unread value is a dash, not a loading bar.
    expect(screen.getAllByText('—').length).toBe(3);
  });

  it('error: a notice gives the reason and what to do', async () => {
    api.status.mockResolvedValue({ ...off, key_set: true, key_tail: 'abcd', notice: 'key_invalid' });
    render(Home);
    await screen.findByText('Gemini API 키가 맞지 않습니다.');
    expect(screen.getByText('설정에서 키를 다시 넣어 주세요.')).toBeTruthy();
  });

  it('full: opens the address in the reader and keeps it among the recent ones', async () => {
    api.status.mockResolvedValue({ ...off, key_set: true, key_tail: 'abcd', source_lang: 'en', gemini_requests: 3 });
    api.openSite.mockResolvedValue(null);
    render(Home);
    await screen.findByText('끝 abcd');
    expect(screen.getByText('영어')).toBeTruthy();
    expect(screen.getByText('3번')).toBeTruthy();
    await typeUrl('https://example.com/manga/1');
    await fireEvent.click(openBar());
    await screen.findByRole('button', { name: 'https://example.com/manga/1' });
    expect(api.openSite).toHaveBeenCalledWith('https://example.com/manga/1');
    expect(JSON.parse(localStorage.getItem('rst-recent-sites'))).toEqual(['https://example.com/manga/1']);
  });

  it('a wrong address is explained', async () => {
    api.status.mockResolvedValue({ ...off, key_set: true, key_tail: 'abcd' });
    api.openSite.mockRejectedValue(new ApiError('bad_url', 500));
    render(Home);
    await screen.findByText('끝 abcd');
    await typeUrl('not a site');
    await fireEvent.click(openBar());
    await screen.findByText(/주소가 맞지 않습니다/);
  });

  it('in a browser, opening says it needs the app and asks for nothing', async () => {
    api.canOpenSites.mockReturnValue(false);
    api.status.mockResolvedValue({ ...off, key_set: true, key_tail: 'abcd' });
    render(Home);
    const why = /브라우저에서는 다른 사이트를 이 화면 안에 열 수 없습니다/;
    await screen.findByText(why);
    await typeUrl('https://example.com/manga/1');
    await fireEvent.click(openBar());
    expect(screen.getAllByText(why).length).toBe(2);
    expect(api.openSite).not.toHaveBeenCalled();
  });
});

const book = (id, pictures) => ({
  id,
  started_at: '2026-11-17T08:00:00Z',
  ended_at: '2026-11-17T09:00:00Z',
  pictures: Array.from({ length: pictures }, (_, i) => ({
    id: i + 1,
    kind: 'stitched',
    preview_path: `strips/${i + 1}/0-1.webp`,
  })),
});

describe('S2 Library', () => {
  it('loading: three grey boxes', () => {
    api.books.mockImplementation(never);
    render(Library);
    const list = feature('list-books')[0];
    expect(list.getAttribute('aria-busy')).toBe('true');
    expect(list.querySelectorAll('[aria-hidden="true"]').length).toBe(3);
  });

  it('empty: guidance and a way back to the first screen', async () => {
    api.books.mockResolvedValue({ books: [], next_before: null });
    render(Library);
    const back = await screen.findByRole('link', { name: '첫 화면으로 가기' });
    expect(back.getAttribute('href')).toBe('/');
  });

  it('error: a 500 says so', async () => {
    api.books.mockImplementation(failing);
    render(Library);
    await screen.findByText(/번역본 목록을 읽지 못했습니다/);
  });

  it('uses the latest search and opens each source through its reader', async () => {
    let finishOldSearch;
    const link = { ...book(3, 0), title: 'Harbor', url: 'https://example.org/harbor' };
    const chapter = { ...book(4, 1), title: 'Harbor folder', imported: true };
    api.books.mockResolvedValueOnce({ books: [], next_before: null })
      .mockImplementationOnce(() => new Promise((resolve) => { finishOldSearch = resolve; }))
      .mockResolvedValueOnce({ books: [link, chapter], next_before: null });
    render(Library);
    await screen.findByRole('link', { name: '첫 화면으로 가기' });
    const search = screen.getByRole('searchbox');
    await fireEvent.input(search, { target: { value: 'h' } });
    await fireEvent.input(search, { target: { value: 'harbor' } });
    await screen.findByText('Harbor folder');
    await act(() => finishOldSearch({ books: [], next_before: null }));
    expect(api.books).toHaveBeenLastCalledWith(null, null, 'harbor');
    expect(screen.getByRole('link', { name: '읽기' }).getAttribute('href')).toBe('/chapter.html?id=4');
    expect(screen.queryByText('링크 저장')).toBeNull();
    await fireEvent.click(screen.getByRole('button', { name: '읽기' }));
    expect(api.openSite).toHaveBeenCalledWith(link.url);
  });

  it('full: exports a book, a failure offers to export again', async () => {
    api.books.mockResolvedValue({ books: [book(2, 2), book(1, 0)], next_before: null });
    api.exportBook.mockRejectedValueOnce(new ApiError('storage_full', 507));
    api.exportBook.mockResolvedValueOnce({ count: 2, files: ['/data/export/a.webp', '/data/export/b.webp'] });
    render(Library);
    await screen.findByText('2장');
    expect(screen.getByText('아직 담긴 그림이 없어 내보낼 것이 없습니다.')).toBeTruthy();
    await fireEvent.click(screen.getByLabelText('WebP'));
    await fireEvent.click(screen.getByRole('button', { name: '내보내기' }));
    await screen.findByText(/저장 공간이 모자랍니다/);
    await fireEvent.click(screen.getByRole('button', { name: '다시 내보내기' }));
    await screen.findByText('그림 2장을 /data/export에 저장했습니다.');
    expect(api.exportBook).toHaveBeenLastCalledWith(2, 'webp');
    expect(screen.queryByRole('link', { name: /내려받기/ })).toBeNull();
  });

  it('full, in a browser: each exported picture is offered as a download', async () => {
    const out = { count: 2, files: ['/data/export/a.png', '/data/export/b.png'] };
    api.books.mockResolvedValue({ books: [book(2, 2)], next_before: null });
    api.exportBook.mockResolvedValue(out);
    api.exportLinks.mockReturnValue([
      { href: '/files/export/a.png', name: 'a.png' },
      { href: '/files/export/b.png', name: 'b.png' },
    ]);
    render(Library);
    await fireEvent.click(await screen.findByRole('button', { name: '내보내기' }));
    await screen.findByText('그림 2장을 만들었습니다. 아래를 눌러 이 기기에 내려받으세요.');
    expect(api.exportLinks).toHaveBeenCalledWith(out);
    const links = screen.getAllByRole('link', { name: /내려받기/ });
    expect(links.map((a) => [a.getAttribute('href'), a.getAttribute('download'), a.dataset.feature])).toEqual([
      ['/files/export/a.png', 'a.png', 'export-book'],
      ['/files/export/b.png', 'b.png', 'export-book'],
    ]);
  });
});

describe('S3 Options', () => {
  it('empty: no key, delete locked', async () => {
    api.status.mockResolvedValue(off);
    render(Options);
    await screen.findByText(/저장된 키가 없습니다/);
    expect(screen.getByRole('button', { name: '키 지우기' }).disabled).toBe(true);
  });

  it('error: a 500 says so', async () => {
    api.status.mockImplementation(failing);
    render(Options);
    await screen.findByText(/저장된 키를 읽지 못했습니다/);
    expect(screen.getByText('알 수 없음')).toBeTruthy();
    expect(screen.queryByText('없음')).toBeNull();
    // A new key can still be put in.
    expect(screen.getByRole('textbox').disabled).toBe(false);
  });

  it('loading: the input is locked while reading and while saving', async () => {
    api.status.mockResolvedValue(off);
    let done;
    api.putKey.mockImplementation(() => new Promise((r) => (done = r)));
    render(Options);
    const input = screen.getByRole('textbox');
    expect(input.disabled).toBe(true);
    await screen.findByText(/저장된 키가 없습니다/);
    expect(input.disabled).toBe(false);
    await fireEvent.input(input, { target: { value: 'AIzaSyExampleKey_1234wxyz' } });
    await fireEvent.click(screen.getByRole('button', { name: '저장' }));
    await screen.findByRole('button', { name: '저장하는 중' });
    expect(input.disabled).toBe(true);
    done({ key_set: true, key_tail: 'wxyz' });
    await screen.findByText('••••wxyz');
    expect(input.disabled).toBe(false);
  });

  it('a bad key is explained under the input and the text kept', async () => {
    api.status.mockResolvedValue(off);
    api.putKey.mockRejectedValue(new ApiError('key_format', 400));
    render(Options);
    await screen.findByText(/저장된 키가 없습니다/);
    const input = screen.getByRole('textbox');
    await fireEvent.input(input, { target: { value: 'bad key' } });
    await fireEvent.click(screen.getByRole('button', { name: '저장' }));
    await screen.findByText(/키 모양이 맞지 않습니다/);
    expect(input.value).toBe('bad key');
  });

  it('chooses the language the manga is written in, and keeps the old one if saving fails', async () => {
    api.status.mockResolvedValue(off);
    api.putSourceLang.mockResolvedValueOnce({ lang: 'en' });
    api.putSourceLang.mockRejectedValueOnce(new ApiError('internal', 500));
    render(Options);
    const ja = await screen.findByRole('radio', { name: /일본어/ });
    await waitFor(() => expect(ja.checked).toBe(true));
    const en = screen.getByRole('radio', { name: /영어/ });
    await fireEvent.click(en);
    await screen.findByText(/새로 여는 페이지부터 이 언어로 읽습니다/);
    expect(api.putSourceLang).toHaveBeenLastCalledWith('en');
    expect(en.checked).toBe(true);
    await fireEvent.click(ja);
    await screen.findByText(/바꾸지 못했습니다/);
    expect(en.checked).toBe(true);
  });

  it('full: saves, then shows only the tail and can delete', async () => {
    api.status.mockResolvedValue(off);
    api.putKey.mockImplementation(async (k) =>
      k ? { key_set: true, key_tail: k.slice(-4) } : { key_set: false, key_tail: null },
    );
    render(Options);
    await screen.findByText(/저장된 키가 없습니다/);
    const input = screen.getByRole('textbox');
    await fireEvent.input(input, { target: { value: 'AIzaSyExampleKey_1234wxyz' } });
    await fireEvent.click(screen.getByRole('button', { name: '저장' }));
    await screen.findByText('••••wxyz');
    expect(input.value).toBe('');
    await fireEvent.click(screen.getByRole('button', { name: '키 지우기' }));
    await screen.findByText('키를 지웠습니다.');
    expect(api.putKey).toHaveBeenLastCalledWith('');
  });
});
