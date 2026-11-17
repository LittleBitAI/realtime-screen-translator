import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
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
      requestPermissions: vi.fn(),
      start: vi.fn(),
      stop: vi.fn(),
      books: vi.fn(),
      exportBook: vi.fn(),
      previewSrc: vi.fn(async (p) => `/files/${p}`),
    },
  };
});

const off = {
  on: false,
  mode: null,
  key_set: false,
  key_tail: null,
  capture_allowed: false,
  overlay_allowed: false,
  notice: null,
  gemini_requests: 0,
};
const never = () => new Promise(() => {});
const failing = async () => {
  throw new ApiError('internal', 500);
};
const feature = (id) => document.querySelectorAll(`[data-feature="${id}"]`);

beforeEach(() => vi.clearAllMocks());
afterEach(cleanup);

describe('S1 Home', () => {
  it('loading: grey bars and a locked toggle', () => {
    api.status.mockImplementation(never);
    render(Home);
    expect(feature('toggle-translation')[0].disabled).toBe(true);
    expect(screen.getByText('상태를 읽는 중입니다.')).toBeTruthy();
    // Not yet known, so neither 켜짐 nor 꺼짐.
    expect(screen.getByText('상태 확인 중')).toBeTruthy();
    expect(screen.queryByText('번역 꺼짐')).toBeNull();
  });

  it('empty: no key, no permissions, asks for the key first', async () => {
    api.status.mockResolvedValue(off);
    render(Home);
    await screen.findByText('번역을 켜려면 Gemini API 키를 먼저 넣어 주세요.');
    expect(screen.getAllByText('없음').length).toBeGreaterThanOrEqual(4);
    expect(screen.getByRole('link', { name: '설정으로 가기' }).getAttribute('href')).toBe('/options');
    await fireEvent.click(feature('toggle-translation')[0]);
    await screen.findByText(/키가 없습니다/);
    expect(api.start).not.toHaveBeenCalled();
  });

  it('error: a 500 says so and offers to read again', async () => {
    api.status.mockImplementation(failing);
    render(Home);
    await screen.findByText(/상태를 읽지 못했습니다/);
    expect(screen.getByRole('button', { name: '다시 읽기' })).toBeTruthy();
    expect(screen.getByText('상태를 알 수 없음')).toBeTruthy();
    expect(screen.queryByText('번역 꺼짐')).toBeNull();
    // Every unread value is a dash, not a loading bar.
    expect(screen.getAllByText('—').length).toBe(6);
  });

  it('error: a notice gives the reason and what to do', async () => {
    api.status.mockResolvedValue({
      ...off,
      on: true,
      key_set: true,
      key_tail: 'abcd',
      capture_allowed: true,
      overlay_allowed: true,
      notice: 'key_invalid',
    });
    render(Home);
    await screen.findByText('Gemini API 키가 맞지 않습니다.');
    expect(screen.getByText('설정에서 키를 다시 넣어 주세요.')).toBeTruthy();
  });

  it('asks for the permissions before turning on, then starts', async () => {
    let s = { ...off, key_set: true, key_tail: 'abcd' };
    api.status.mockImplementation(async () => s);
    api.requestPermissions.mockImplementation(async () => {
      s = { ...s, capture_allowed: true, overlay_allowed: true };
      return { capture_allowed: true, overlay_allowed: true };
    });
    api.start.mockImplementation(async () => {
      s = { ...s, on: true, mode: 'scroll', gemini_requests: 3 };
      return s;
    });
    render(Home);
    // The toggle reads 번역 켜기 while loading too: wait for the status.
    await screen.findByText('끝 abcd');
    await fireEvent.click(feature('toggle-translation')[0]);
    await screen.findByText('번역 끄기');
    expect(api.requestPermissions).toHaveBeenCalledOnce();
    expect(api.start).toHaveBeenCalledOnce();
    // full: on, the mode and the request count, the permissions collapsed.
    expect(screen.getByText('번역 켜짐')).toBeTruthy();
    expect(screen.getByText('스크롤')).toBeTruthy();
    expect(screen.getByText('3번')).toBeTruthy();
    expect(screen.getByText('권한 모두 허용됨').closest('details').open).toBe(false);
  });

  it('a refusal explains why and offers to allow again', async () => {
    api.status.mockResolvedValue({ ...off, key_set: true, key_tail: 'abcd' });
    api.requestPermissions.mockResolvedValue({ capture_allowed: false, overlay_allowed: false });
    render(Home);
    const ask = await screen.findByRole('button', { name: '권한 허용하기' });
    await fireEvent.click(ask);
    await screen.findByText(/허용하지 않으셨습니다/);
    expect(screen.getByRole('button', { name: '권한 허용하기' }).disabled).toBe(false);
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
