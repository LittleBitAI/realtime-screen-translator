<script>
  import { onMount } from 'svelte';
  import Shell from '../lib/Shell.svelte';
  import { api } from '../lib/api.js';
  import { link, navigate } from '../lib/router.svelte.js';
  import { importError, langText, notices, openError, reachError, readerUnsupported } from '../lib/text.js';

  // S1: open a manga site in the reader, and see at a glance whether the
  // key and Gemini are all right. The translation is drawn on the site's
  // page pictures themselves (P12), so nothing here turns anything on or off.
  const RECENT = 'rst-recent-sites';
  let status = $state(null);
  let failed = $state(false);
  let busy = $state(false);
  let importing = $state(false);
  let mounted = false;
  let problem = $state(null);
  let recent = $state(readRecent());
  let url = $state(recent[0] ?? '');

  function readRecent() {
    try {
      const list = JSON.parse(localStorage.getItem(RECENT) ?? '[]');
      return Array.isArray(list) ? list.filter((u) => typeof u === 'string').slice(0, 5) : [];
    } catch {
      return [];
    }
  }

  async function refresh() {
    try {
      status = await api.status();
      failed = false;
    } catch {
      failed = true;
    }
  }

  onMount(() => {
    mounted = true;
    refresh();
    const timer = setInterval(refresh, 2000);
    return () => {
      mounted = false;
      clearInterval(timer);
    };
  });

  let unsupported = !api.canOpenSites();
  let notice = $derived(status?.notice ? notices[status.notice] : null);

  async function open(address = url) {
    const typed = address.trim();
    if (busy || importing || !typed) return;
    problem = null;
    if (unsupported) {
      problem = { text: readerUnsupported };
      return;
    }
    if (status && !status.key_set) {
      problem = { text: openError('key_missing'), options: true };
      return;
    }
    busy = true;
    try {
      await api.openSite(typed);
      recent = [typed, ...recent.filter((u) => u !== typed)].slice(0, 5);
      localStorage.setItem(RECENT, JSON.stringify(recent));
    } catch (e) {
      problem = { text: openError(e.code) };
    } finally {
      busy = false;
    }
  }

  function submit(e) {
    e.preventDefault();
    open();
  }

  async function importFolder() {
    if (busy || importing) return;
    importing = true;
    problem = null;
    try {
      const id = await api.importChapter();
      if (mounted && id != null) navigate('/library');
    } catch (e) {
      if (mounted) problem = { text: importError(e.code) };
    } finally {
      if (mounted) importing = false;
    }
  }
</script>

{#snippet bar()}
  <button
    type="submit"
    form="open-site"
    data-feature="open-site"
    disabled={busy || importing || !url.trim()}
    class={['h-[72px] w-full rounded-2xl text-xl font-bold', busy || importing || !url.trim() ? 'bg-gray-400 text-white' : 'bg-primary text-white']}
  >
    {busy ? '여는 중' : '열기'}
  </button>
{/snippet}

<!-- A value not read yet: a grey bar while loading, a dash once the read failed. -->
{#snippet blank(width)}
  {#if failed}<span class="text-gray-500">—</span>{:else}<span class={['inline-block h-4 rounded bg-gray-300', width]} aria-hidden="true"></span>{/if}
{/snippet}

<Shell title="만화 번역" note="{status ? langText(status.source_lang) : '원문'} → 한국어" current="/" {bar}>
  <div class="flex flex-wrap content-start items-start gap-4">
    <section class="box-border min-w-0 flex-[1_1_300px] rounded-xl border border-gray-300 bg-white p-4">
      <h2 class="m-0 text-xl font-bold">만화 페이지 열기</h2>
      <!-- novalidate: an address typed without https:// is fine (the app adds it). -->
      <form id="open-site" onsubmit={submit} novalidate class="mt-4">
        <label for="site-url" class="block font-semibold">주소</label>
        <input
          id="site-url"
          type="url"
          inputmode="url"
          autocomplete="url"
          enterkeyhint="go"
          placeholder="https://"
          bind:value={url}
          class="mt-2 box-border min-h-12 w-full rounded-lg border border-gray-500 px-3 text-base"
        />
      </form>
      <p class="m-0 mt-2 text-sm text-gray-500">
        열린 페이지의 말풍선 위에 한국어가 붙고, 스크롤하거나 페이지를 넘겨도 그림과 함께 움직입니다. 한 번 번역한 페이지는 다시 번역하지 않습니다.
        페이지 아래 <b>앱</b> 버튼이나 뒤로 가기로 돌아옵니다.
      </p>
      {#if unsupported}
        <p class="m-0 mt-4 rounded-lg bg-gray-200 px-4 py-2 text-sm">{readerUnsupported}</p>
      {:else}
        <button
          type="button"
          data-feature="import-chapter"
          onclick={importFolder}
          disabled={busy || importing}
          class="mt-4 min-h-12 w-full rounded-lg border border-primary px-4 font-semibold text-primary disabled:opacity-50"
        >{importing ? '가져오는 중' : '그림 폴더 가져오기'}</button>
      {/if}
      {#if recent.length}
        <h3 class="m-0 mt-4 text-base font-bold">최근 연 주소</h3>
        <ul class="m-0 mt-2 flex list-none flex-col gap-2 p-0">
          {#each recent as r (r)}
            <li>
              <button
                type="button"
                onclick={() => {
                  url = r;
                  open(r);
                }}
                disabled={busy || importing}
                class="box-border min-h-12 w-full truncate rounded-lg border border-gray-300 bg-gray-50 px-3 text-left text-sm"
              >{r}</button>
            </li>
          {/each}
        </ul>
      {/if}
    </section>

    <section
      data-feature="translation-status"
      aria-busy={!status && !failed}
      class="box-border min-w-0 flex-[1_1_300px] rounded-xl border border-gray-300 bg-white p-4"
    >
      <h2 class="m-0 text-xl font-bold">번역 상태</h2>
      <div role="status" class="mt-4 rounded-lg px-4 py-2 text-sm" class:bg-red-50={failed || problem || notice} class:bg-gray-200={!(failed || problem || notice)}>
        {#if failed}
          <p class="m-0">상태를 읽지 못했습니다. {reachError()}</p>
          <button type="button" onclick={refresh} class="mt-2 min-h-12 rounded-lg border border-gray-500 bg-white px-4">다시 읽기</button>
        {:else if problem}
          <p class="m-0">{problem.text}</p>
          {#if problem.options}<a href="/options" onclick={link} class="mt-2 inline-flex min-h-12 items-center rounded-lg bg-primary px-4 text-white no-underline">설정으로 가기</a>{/if}
        {:else if notice}
          <p class="m-0 font-semibold">{notice.why}</p>
          <p class="m-0">{notice.todo}</p>
          {#if notice.options}<a href="/options" onclick={link} class="mt-2 inline-flex min-h-12 items-center rounded-lg bg-primary px-4 text-white no-underline">설정으로 가기</a>{/if}
        {:else if !status}
          <p class="m-0">상태를 읽는 중입니다.</p>
        {:else if !status.key_set}
          <p class="m-0">번역하려면 Gemini API 키를 먼저 넣어 주세요.</p>
          <a href="/options" onclick={link} class="mt-2 inline-flex min-h-12 items-center rounded-lg bg-primary px-4 text-white no-underline">설정으로 가기</a>
        {:else}
          <p class="m-0">키나 Gemini 연결에 문제가 생기면 여기와 열린 페이지 아래에 알려 드립니다.</p>
        {/if}
      </div>

      <dl class="m-0 mt-4 flex flex-col gap-2">
        <div class="flex items-center justify-between gap-2">
          <dt>Gemini API 키</dt>
          <dd class="m-0 text-right">{#if status}{status.key_set ? `끝 ${status.key_tail}` : '없음'}{:else}{@render blank('w-18')}{/if}</dd>
        </div>
        <div class="flex items-center justify-between gap-2">
          <dt>원문 언어</dt>
          <dd class="m-0 text-right">
            {#if status}{langText(status.source_lang)}
              <a href="/options" onclick={link} class="ml-2 inline-flex min-h-12 items-center text-primary">바꾸기</a>{:else}{@render blank('w-18')}{/if}
          </dd>
        </div>
        <div class="flex items-center justify-between gap-2">
          <dt>앱을 연 뒤 Gemini 요청</dt>
          <dd class="m-0 text-right">{#if status}{status.gemini_requests}번{:else}{@render blank('w-12')}{/if}</dd>
        </div>
      </dl>
    </section>
  </div>
</Shell>
