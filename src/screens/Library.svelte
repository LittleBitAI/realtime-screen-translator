<script>
  import { onMount, tick } from 'svelte';
  import Shell from '../lib/Shell.svelte';
  import { api } from '../lib/api.js';
  import { link } from '../lib/router.svelte.js';
  import { bookName, exportDone, exportError, exportReady, kindText, openError, reachError } from '../lib/text.js';

  // S2: the books on this device, newest first, each exported on its own.
  let books = $state(null);
  let failed = $state(false);
  let next = $state(null);
  let more = $state(false);
  let lists = $state(null);
  let list = $state(null);
  let search = $state('');
  let wide = $state(false);
  let expanded = $state(true);
  let addingLink = $state(false);
  let linkToggle = $state();
  let editing = $state(false);
  let saving = $state(false);
  let message = $state('');
  let linkTitle = $state('');
  let address = $state('');
  let newName = $state('');
  let rename = $state('');
  let deleting = $state(false);
  let selected = $derived(lists?.find((l) => l.id === list));
  let generation = 0;
  const field = 'box-border min-h-12 min-w-0 rounded-lg border border-control bg-white px-3 text-base';
  const button = 'min-h-12 rounded-lg border border-control bg-white px-3 font-semibold disabled:opacity-50';
  /** Per book id: { format, busy, error, done }. */
  let jobs = $state({});

  async function load(refreshLists = false) {
    const current = ++generation;
    failed = false;
    books = null;
    next = null;
    more = false;
    try {
      const [out, names] = await Promise.all([
        api.books(null, list, search), refreshLists || !lists ? api.lists() : lists,
      ]);
      if (current !== generation) return;
      lists = names;
      books = out.books;
      next = out.next_before;
    } catch {
      if (current === generation) failed = true;
    }
  }

  async function loadMore() {
    const current = generation;
    more = true;
    try {
      const out = await api.books(next, list, search);
      if (current !== generation) return;
      books = [...books, ...out.books];
      next = out.next_before;
    } catch {
      if (current === generation) failed = true;
    } finally {
      if (current === generation) more = false;
    }
  }

  onMount(() => {
    const query = window.matchMedia?.('(min-width: 62rem)');
    const updateLayout = () => { wide = query?.matches ?? false; };
    updateLayout();
    query?.addEventListener('change', updateLayout);
    load(true);
    return () => {
      query?.removeEventListener('change', updateLayout);
      generation++;
    };
  });

  function selectList(id) {
    if (id === list) {
      if (!wide) expanded = !expanded;
      return;
    }
    list = id;
    expanded = true;
    rename = lists.find((l) => l.id === id)?.name ?? '';
    deleting = false;
    message = '';
    load();
  }

  async function edit(change) {
    if (saving) return;
    const current = generation;
    let returnFocus = false;
    saving = true;
    message = '';
    try {
      await api.editLibrary(change);
      if (current !== generation) return;
      if (change.action === 'create') newName = '';
      if (change.action === 'delete' || change.action === 'save_link') {
        list = null;
        expanded = true;
        deleting = false;
      }
      if (change.action === 'save_link') {
        search = address = linkTitle = '';
        addingLink = false;
        returnFocus = true;
      }
      await load(true);
    } catch (e) {
      if (current === generation) message = e.code === 'bad_url' ? openError(e.code) : '변경을 저장하지 못했습니다. 다시 시도해 주세요.';
    } finally {
      saving = false;
      if (returnFocus) {
        await tick();
        linkToggle?.focus();
      }
    }
  }

  async function openLink(url) {
    const current = generation;
    message = '';
    try {
      await api.openSite(url);
    } catch (e) {
      if (current === generation) message = openError(e.code);
    }
  }

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

{#snippet listButton(id, name, count)}
  {@const active = list === id}
  <button
    type="button"
    disabled={saving}
    aria-pressed={wide ? active : undefined}
    aria-expanded={wide ? undefined : active && expanded}
    aria-controls={wide || (active && expanded) ? 'library-titles' : undefined}
    onclick={() => selectList(id)}
    class={[
      'flex min-h-12 w-full items-center gap-2 rounded-lg border px-3 py-2 text-start font-semibold disabled:opacity-50',
      active && (wide || expanded) ? 'border-primary bg-selection text-primary' : 'border-gray-300 bg-white text-ink hover:bg-gray-50',
    ]}
  >
    <span class="min-w-0 flex-1 wrap-anywhere">{name}</span>
    {#if count !== undefined}<span class="shrink-0 text-sm font-normal tabular-nums text-muted">{count}</span>{/if}
    {#if !wide}<span class="list-chevron shrink-0" class:expanded={active && expanded} aria-hidden="true"></span>{/if}
  </button>
{/snippet}

{#snippet allTitles()}
  <li>
    {@render listButton(null, '전체 작품', undefined)}
    {#if !wide && list === null && expanded}{@render titles()}{/if}
  </li>
{/snippet}

<Shell title="번역본" current="/library">
  <fieldset disabled={saving} class="m-0 flex min-w-0 flex-wrap items-end gap-2 border-0 p-0">
    <label class="grid min-w-0 flex-[1_1_100%] gap-1 text-sm font-semibold min-[62rem]:max-w-xl min-[62rem]:flex-[1_1_18rem]">제목 또는 주소 검색
      <input type="search" class={field} value={search} oninput={(e) => { search = e.currentTarget.value; expanded = true; load(); }} />
    </label>
    <button type="button" class={button} bind:this={linkToggle} aria-expanded={addingLink} onclick={() => { addingLink = !addingLink; message = ''; }}>{addingLink ? '링크 추가 닫기' : '링크 추가'}</button>
    <button type="button" class={button} disabled={!lists} aria-expanded={editing} onclick={() => { editing = !editing; rename = selected?.name ?? ''; deleting = false; }}>{editing ? '편집 닫기' : '편집'}</button>
  </fieldset>
  {#if addingLink}
    <form class="mt-4 rounded-xl border border-gray-300 bg-white p-4" onsubmit={(e) => { e.preventDefault(); edit({ action: 'save_link', title: linkTitle, url: address }); }}>
      <fieldset disabled={saving} class="m-0 flex min-w-0 flex-wrap items-end gap-2 border-0 p-0">
        <label class="grid min-w-0 flex-[1_1_14rem] gap-1 text-sm">작품 제목<input class={field} bind:value={linkTitle} required maxlength="120" /></label>
        <label class="grid min-w-0 flex-[2_1_20rem] gap-1 text-sm">사이트 주소<input class={field} bind:value={address} inputmode="url" required placeholder="https://" /></label>
        <button class={button} disabled={!linkTitle.trim() || !address.trim()}>링크 저장</button>
      </fieldset>
    </form>
  {/if}
  {#if editing}
    <fieldset disabled={saving} class="m-0 mt-4 grid min-w-0 gap-4 rounded-xl border border-gray-300 bg-white p-4">
      <form class="flex flex-wrap items-end gap-2" onsubmit={(e) => { e.preventDefault(); edit({ action: 'create', name: newName }); }}>
        <label class="grid min-w-0 flex-1 gap-1 text-sm">새 목록 이름<input class={field} bind:value={newName} required maxlength="120" /></label>
        <button class={button} disabled={!newName.trim()}>목록 만들기</button>
      </form>
      {#if selected}
        <form class="flex flex-wrap items-end gap-2" onsubmit={(e) => { e.preventDefault(); edit({ action: 'rename', list, name: rename }); }}>
          <label class="grid min-w-0 flex-1 gap-1 text-sm">선택한 목록 이름<input class={field} bind:value={rename} required maxlength="120" /></label>
          <button class={button} disabled={!rename.trim() || rename.trim() === selected.name}>이름 변경</button>
        </form>
        <div class="flex flex-wrap gap-2">
          <button class={button} disabled={lists[0].id === list} onclick={() => edit({action: 'move_list', list, up: true})}>목록 위로</button>
          <button class={button} disabled={lists.at(-1).id === list} onclick={() => edit({action: 'move_list', list, up: false})}>목록 아래로</button>
          <button class={button} onclick={() => deleting = !deleting}>{deleting ? '취소' : '목록 삭제'}</button>
          {#if deleting}
            <p class="m-0 w-full text-sm">‘{selected.name}’ 목록을 삭제할까요? 작품과 번역본은 보관됩니다.</p>
            <button class={button} onclick={() => edit({ action: 'delete', list })}>이 목록 삭제</button>
          {/if}
        </div>
      {/if}
    </fieldset>
  {/if}
  {#if message}<p role="alert" class="rounded-lg bg-red-50 p-2">{message}</p>{/if}

  <div class="mt-6 grid items-start gap-6 min-[62rem]:grid-cols-[17rem_minmax(0,1fr)]">
    <section aria-labelledby="library-lists" class="min-w-0 min-[62rem]:sticky min-[62rem]:top-22">
      <h2 id="library-lists" class="mb-3 text-xl leading-[1.3] font-bold">목록</h2>
      <ul class="m-0 grid list-none gap-2 p-0 min-[62rem]:-m-1 min-[62rem]:max-h-[calc(100dvh-8rem)] min-[62rem]:overflow-y-auto min-[62rem]:p-1">
        {#if wide}{@render allTitles()}{/if}
        {#each lists ?? [] as saved (saved.id)}
          <li>
            {@render listButton(saved.id, saved.name, saved.entries.length)}
            {#if !wide && list === saved.id && expanded}{@render titles()}{/if}
          </li>
        {/each}
        {#if !wide}{@render allTitles()}{/if}
      </ul>
    </section>
    {#if wide}{@render titles()}{/if}
  </div>
</Shell>

{#snippet titles()}
  <section id="library-titles" aria-labelledby="library-titles-heading" class="min-w-0 pt-2 min-[62rem]:pt-0">
    <h2 id="library-titles-heading" class="sr-only text-xl leading-[1.3] font-bold wrap-anywhere min-[62rem]:not-sr-only">{selected?.name ?? '전체 작품'}</h2>
    {#if failed && !books}
      <div role="alert" class="rounded-lg bg-red-50 px-4 py-2 text-sm min-[62rem]:mt-3">
        <p class="m-0">번역본 목록을 읽지 못했습니다. {reachError()}</p>
        <button type="button" data-feature="list-books" onclick={() => load(true)} class="mt-2 min-h-12 rounded-lg border border-control bg-white px-4">다시 읽기</button>
      </div>
    {:else if !books}
      <div data-feature="list-books" aria-busy="true" class="title-grid">
        {#each [1, 2, 3] as n (n)}
          <div class="h-32 min-w-0 rounded-xl border border-gray-300 bg-gray-200" aria-hidden="true"></div>
        {/each}
        <span class="sr-only">번역본 목록을 읽는 중입니다.</span>
      </div>
    {:else if books.length === 0 && (list != null || search.trim())}
      <p role="status" class="m-0 rounded-xl border border-gray-300 bg-white p-4 min-[62rem]:mt-3">{search.trim() ? '검색 결과가 없습니다.' : '이 목록에 작품이 없습니다. 전체 작품에서 편집을 열어 추가하세요.'}</p>
    {:else if books.length === 0}
      <div data-feature="list-books" class="rounded-xl border border-gray-300 bg-white p-4 min-[62rem]:mt-3">
        <h3 class="m-0 text-base font-semibold">아직 저장된 번역본이 없습니다</h3>
        <p class="m-0 mt-2 text-sm text-muted">만화를 읽거나 링크를 추가하면 여기에 작품이 모입니다.</p>
        <a href="/" onclick={link} class="mt-4 inline-flex min-h-12 items-center rounded-lg bg-primary px-4 font-semibold text-white no-underline">첫 화면으로 가기</a>
      </div>
    {:else}
      <div data-feature="list-books" class="title-grid">
        {#each books as book (book.id)}
          {@const j = jobs[book.id] ?? { format: 'png', busy: false, error: '', done: '', links: [] }}
          <article class="min-w-0 rounded-xl border border-gray-300 bg-white p-4">
            <div class="flex items-start gap-3">
              {#if preview(book)}
                <div class="aspect-[3/4] w-18 shrink-0 overflow-hidden rounded-lg bg-gray-100">
                  {#await api.previewSrc(preview(book)) then src}
                    <img {src} alt="" loading="lazy" class="h-full w-full object-cover object-top" />
                  {:catch}
                    <span class="block p-2 text-sm text-muted">미리보기 없음</span>
                  {/await}
                </div>
              {/if}
              <div class="min-w-0">
                <h3 class="m-0 text-base font-semibold wrap-anywhere">{bookName(book)}</h3>
                {#if book.url}<p class="m-0 mt-1 text-sm wrap-anywhere text-muted">{book.url}</p>
                {:else if !book.imported}<p class="m-0 mt-1 text-sm text-muted">원본 주소 없음</p>{/if}
                <div class="mt-2 flex flex-wrap items-center gap-x-2 gap-y-1 text-sm text-muted">
                  <span class="tabular-nums">{book.pictures.length}장</span>
                  {#each kinds(book) as kind (kind)}<span>{kindText(kind)}</span>{/each}
                  {#if book.ended_at == null}<span class="rounded bg-selection px-2 py-1 text-accent">지금 켜져 있는 번역본</span>{/if}
                </div>
              </div>
            </div>
            {#if book.imported && api.canOpenSites()}
              <a
                href="/chapter.html?id={book.id}"
                class="mt-4 inline-flex min-h-12 w-full items-center justify-center rounded-lg border border-primary px-4 font-semibold text-primary no-underline min-[62rem]:w-auto"
              >읽기</a>
            {:else if book.url && api.canOpenSites()}
              <button class="mt-4 min-h-12 w-full rounded-lg border border-primary px-4 font-semibold text-primary min-[62rem]:w-auto" onclick={() => openLink(book.url)}>읽기</button>
            {/if}
            {#if editing && lists?.length}
              <fieldset disabled={saving} class="m-0 mt-4 flex min-w-0 flex-wrap gap-2 border-0 p-0">
                <legend class="text-sm">작품을 담을 목록</legend>
                {#each lists as l (l.id)}
                  <label class="inline-flex min-h-12 items-center gap-2 rounded-lg border border-gray-300 px-3 text-sm wrap-anywhere">
                    <input type="checkbox" class="size-5 shrink-0" checked={l.entries.includes(book.id)} onchange={(e) => {
                      const add = e.currentTarget.checked;
                      e.currentTarget.checked = !add;
                      edit({ action: add ? 'add' : 'remove', list: l.id, book: book.id });
                    }} />{l.name}
                  </label>
                {/each}
                {#if selected && !search.trim()}
                  <div class="flex w-full flex-wrap gap-2">
                    <button class={button} disabled={selected.entries[0] === book.id} onclick={() => edit({action: 'move_entry', list, book: book.id, up: true})}>작품 위로</button>
                    <button class={button} disabled={selected.entries.at(-1) === book.id} onclick={() => edit({action: 'move_entry', list, book: book.id, up: false})}>작품 아래로</button>
                  </div>
                {/if}
              </fieldset>
            {/if}
            {#if book.pictures.length === 0}
              {#if !book.url}<p class="m-0 mt-3 text-sm text-muted">아직 담긴 그림이 없어 내보낼 것이 없습니다.</p>{/if}
            {:else}
              <details class="mt-3 rounded-lg border border-gray-300">
                <summary class="min-h-12 cursor-pointer rounded-lg px-3 py-3 font-semibold">그림 내보내기</summary>
                <div class="px-3 pb-3">
                  <fieldset class="m-0 grid grid-cols-2 gap-2 border-0 p-0">
                    <legend class="mb-2 p-0 text-sm text-muted">파일 형식</legend>
                    {#each [['png', 'PNG'], ['webp', 'WebP']] as [value, label] (value)}
                      <label
                        class={[
                          'flex min-h-12 cursor-pointer items-center justify-center rounded-lg border has-[:focus-visible]:outline-2 has-[:focus-visible]:outline-accent',
                          j.format === value ? 'border-2 border-primary font-semibold text-ink' : 'border-control text-muted',
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
                  {#if j.done}<p role="status" class="m-0 mt-2 rounded-lg bg-selection px-4 py-2 text-sm break-all">{j.done}</p>{/if}
                  {#if j.links.length}
                    <ul class="m-0 mt-2 flex list-none flex-col gap-2 p-0">
                      {#each j.links as file, i (file.href)}
                        <li>
                          <a
                            href={file.href}
                            download={file.name}
                            data-feature="export-book"
                            class="flex min-h-12 items-center justify-center rounded-lg border border-primary px-4 font-semibold text-ink no-underline"
                          >
                            그림 {i + 1} 내려받기
                          </a>
                        </li>
                      {/each}
                    </ul>
                  {/if}
                </div>
              </details>
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
          class="mt-4 min-h-12 w-full rounded-lg border border-control bg-gray-200 text-ink"
        >
          {more ? '읽는 중' : '번역본 더 보기'}
        </button>
      {/if}
      {#if api.canOpenSites() && books.some((book) => book.pictures.length)}
        <p class="m-0 mt-4 text-sm text-muted">내보낸 그림은 휴대폰에서는 갤러리에, PC에서는 내려받기 폴더에 저장됩니다.</p>
      {/if}
    {/if}
  </section>
{/snippet}

<style>
  .title-grid {
    display: grid;
    gap: .75rem;
    align-items: start;
  }

  .list-chevron {
    width: .5rem;
    height: .5rem;
    margin-inline: .25rem;
    border-inline-end: 2px solid currentColor;
    border-block-end: 2px solid currentColor;
    transform: rotate(-45deg);
  }

  .list-chevron.expanded { transform: rotate(45deg); }

  @media (min-width: 62rem) {
    .title-grid {
      margin-block-start: .75rem;
      grid-template-columns: repeat(auto-fill, minmax(min(100%, 23rem), 1fr));
      gap: 1rem;
    }
  }
</style>
