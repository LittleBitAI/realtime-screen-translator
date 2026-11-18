<script>
  import { onMount } from 'svelte';
  import Shell from '../lib/Shell.svelte';
  import { api } from '../lib/api.js';
  import { link } from '../lib/router.svelte.js';
  import { bookName, exportDone, exportError, exportReady, kindText, reachError } from '../lib/text.js';

  // The books and the boxes standing in for them while loading share one grid.
  const grid = 'mt-4 grid grid-cols-1 items-start gap-4 sm:grid-cols-2';

  // S2: the books on this device, newest first, each exported on its own.
  let books = $state(null);
  let failed = $state(false);
  let next = $state(null);
  let more = $state(false);
  /** Per book id: { format, busy, error, done }. */
  let jobs = $state({});

  async function load() {
    failed = false;
    try {
      const out = await api.books();
      books = out.books;
      next = out.next_before;
    } catch {
      failed = true;
    }
  }

  async function loadMore() {
    more = true;
    try {
      const out = await api.books(next);
      books = [...books, ...out.books];
      next = out.next_before;
    } catch {
      failed = true;
    } finally {
      more = false;
    }
  }

  onMount(load);

  // The entry read back from the state, so changes to it are seen.
  function job(id) {
    jobs[id] ??= { format: 'png', busy: false, error: '', done: '', links: [] };
    return jobs[id];
  }

  async function exportBook(book) {
    const j = job(book.id);
    if (j.busy) return;
    j.busy = true;
    j.error = '';
    j.done = '';
    j.links = [];
    try {
      const out = await api.exportBook(book.id, j.format);
      // In a browser the files sit on the host: the reader downloads each one.
      j.links = api.exportLinks(out);
      j.done = j.links.length ? exportReady(out.count) : exportDone(out);
    } catch (e) {
      j.error = exportError(e.code);
    } finally {
      j.busy = false;
    }
  }

  const kinds = (book) => [...new Set(book.pictures.map((p) => p.kind))];
  const preview = (book) => book.pictures.find((p) => p.preview_path)?.preview_path;
</script>

<Shell title="번역본" note="새것부터" current="/library">
  <p class="m-0 rounded-lg bg-gray-200 px-4 py-2 text-sm">내보낸 그림은 휴대폰에서는 갤러리에, PC에서는 내려받기 폴더에 저장됩니다.</p>

  {#if failed && !books}
    <div role="alert" class="mt-4 rounded-lg bg-red-50 px-4 py-2 text-sm">
      <p class="m-0">번역본 목록을 읽지 못했습니다. {reachError()}</p>
      <button type="button" data-feature="list-books" onclick={load} class="mt-2 min-h-12 rounded-lg border border-gray-500 bg-white px-4">다시 읽기</button>
    </div>
  {:else if !books}
    <div data-feature="list-books" aria-busy="true" class={grid}>
      {#each [1, 2, 3] as n (n)}
        <div class="box-border h-[452px] min-w-0 rounded-xl border border-gray-300 bg-gray-200" aria-hidden="true"></div>
      {/each}
      <span class="sr-only">번역본 목록을 읽는 중입니다.</span>
    </div>
  {:else if books.length === 0}
    <div data-feature="list-books" class="mt-4 rounded-xl border border-gray-300 bg-white p-4">
      <h2 class="m-0 text-xl font-bold">아직 저장된 번역본이 없습니다</h2>
      <p class="m-0 mt-2">첫 화면에서 만화 페이지를 열어 읽으면, 번역이 얹힌 그림이 여기에 번역본으로 쌓입니다.</p>
      <a href="/" onclick={link} class="mt-4 inline-flex min-h-12 items-center rounded-lg bg-primary px-4 font-semibold text-white no-underline">첫 화면으로 가기</a>
    </div>
  {:else}
    <div data-feature="list-books" class={grid}>
      {#each books as book (book.id)}
        {@const j = jobs[book.id] ?? { format: 'png', busy: false, error: '', done: '', links: [] }}
        <article class="box-border min-w-0 rounded-xl border border-gray-300 bg-white p-4">
          <div class="flex h-40 items-center justify-center overflow-hidden rounded-lg bg-gray-300 text-sm text-gray-500">
            {#if preview(book)}
              {#await api.previewSrc(preview(book))}
                미리보기
              {:then src}
                <img {src} alt="{bookName(book)} 미리보기" class="h-full w-full object-cover object-top" />
              {:catch}
                미리보기 없음
              {/await}
            {:else}
              미리보기 없음
            {/if}
          </div>
          <h2 class="m-0 mt-4 text-base font-bold break-keep">{bookName(book)}</h2>
          {#if book.imported && api.canOpenSites()}
            <a
              href="/chapter.html?id={book.id}"
              class="mt-4 inline-flex min-h-12 w-full box-border items-center justify-center rounded-lg border border-primary px-4 font-semibold text-primary no-underline"
            >읽기</a>
          {/if}
          <dl class="m-0 mt-2 flex flex-col gap-2">
            <div class="flex items-center justify-between gap-2">
              <dt>그림 수</dt>
              <dd class="m-0">{book.pictures.length}장</dd>
            </div>
          </dl>
          <div class="mt-2 flex flex-wrap gap-2 text-sm">
            {#each kinds(book) as kind (kind)}<span class="rounded bg-gray-200 px-2 py-1">{kindText(kind)}</span>{/each}
            {#if book.ended_at == null}<span class="rounded bg-blue-50 px-2 py-1 text-accent">지금 켜져 있는 번역본</span>{/if}
          </div>
          {#if book.pictures.length === 0}
            <p class="m-0 mt-4 text-sm text-gray-500">아직 담긴 그림이 없어 내보낼 것이 없습니다.</p>
          {:else}
            <fieldset class="m-0 mt-4 grid grid-cols-2 gap-2 border-0 p-0">
              <legend class="mb-2 p-0 text-sm text-gray-500">파일 형식</legend>
              {#each [['png', 'PNG'], ['webp', 'WebP']] as [value, label] (value)}
                <label
                  class={[
                    'flex min-h-11 cursor-pointer items-center justify-center rounded-lg border has-[:focus-visible]:outline-2 has-[:focus-visible]:outline-accent',
                    j.format === value ? 'border-2 border-primary font-semibold text-ink' : 'border-gray-300 text-gray-500',
                  ]}
                >
                  <input
                    type="radio"
                    name="format-{book.id}"
                    {value}
                    checked={j.format === value}
                    data-feature="export-book"
                    onchange={() => (job(book.id).format = value)}
                    class="sr-only"
                  />{label}
                </label>
              {/each}
            </fieldset>
            <button
              type="button"
              data-feature="export-book"
              disabled={j.busy}
              onclick={() => exportBook(book)}
              class="mt-4 min-h-12 w-full rounded-lg bg-primary font-semibold text-white disabled:bg-gray-400"
            >
              {#if j.busy}내보내는 중{:else if j.error}다시 내보내기{:else}내보내기{/if}
            </button>
            {#if j.error}<p role="alert" class="m-0 mt-2 rounded-lg bg-red-50 px-4 py-2 text-sm">{j.error}</p>{/if}
            {#if j.done}<p role="status" class="m-0 mt-2 rounded-lg bg-blue-50 px-4 py-2 text-sm break-all">{j.done}</p>{/if}
            {#if j.links.length}
              <ul class="m-0 mt-2 flex list-none flex-col gap-2 p-0">
                {#each j.links as file, i (file.href)}
                  <li>
                    <a
                      href={file.href}
                      download={file.name}
                      data-feature="export-book"
                      class="flex min-h-11 items-center justify-center rounded-lg border border-primary px-4 font-semibold text-ink no-underline"
                    >
                      그림 {i + 1} 내려받기
                    </a>
                  </li>
                {/each}
              </ul>
            {/if}
          {/if}
        </article>
      {/each}
    </div>
    {#if failed}
      <p role="alert" class="m-0 mt-4 rounded-lg bg-red-50 px-4 py-2 text-sm">번역본을 더 읽지 못했습니다. {reachError()}</p>
    {/if}
    {#if next != null}
      <button
        type="button"
        data-feature="list-books"
        disabled={more}
        onclick={loadMore}
        class="mt-4 min-h-12 w-full rounded-lg border border-gray-500 bg-gray-200 text-ink"
      >
        {more ? '읽는 중' : '번역본 더 보기'}
      </button>
    {/if}
  {/if}
</Shell>
