<script>
  import { onMount } from 'svelte';
  import Shell from '../lib/Shell.svelte';
  import { api } from '../lib/api.js';
  import { keyError, reachError } from '../lib/text.js';

  // S3: put in, change or delete one's own Gemini API key.

  /** How translation works, fixed by the spec: name, value, and a plain note where the name needs one. */
  const fixed = [
    ['번역 모델', 'Gemini 3.1 Flash Lite'],
    ['옮기는 방향', '일본어 → 한국어'],
    ['내용 검열', '하지 않음', '거친 대사나 싸움 장면도 빼지 않고 그대로 옮깁니다.'],
    ['번역 말투 고정', '켬', '답의 첫머리를 정해 두어 대사의 번역만 같은 꼴로 돌아옵니다.'],
    ['효과음', '번역하지 않음'],
  ];
  let tail = $state(undefined);
  let failed = $state(false);
  let key = $state('');
  let busy = $state(null);
  let error = $state('');
  let saved = $state('');

  async function load() {
    failed = false;
    try {
      const s = await api.status();
      tail = s.key_set ? s.key_tail : null;
    } catch {
      failed = true;
    }
  }

  onMount(load);

  async function put(value, what) {
    busy = what;
    error = '';
    saved = '';
    try {
      const out = await api.putKey(value);
      tail = out.key_set ? out.key_tail : null;
      key = '';
      saved = out.key_set ? '키를 저장했습니다. 이제 번역을 켤 수 있습니다.' : '키를 지웠습니다.';
    } catch (e) {
      // What was typed stays, to be checked and fixed.
      error = keyError(e.code);
    } finally {
      busy = null;
    }
  }

  // The input and 저장 wait for the first read and for a save under way. A
  // failed read leaves them open: a new key can still be put in.
  let locked = $derived(!!busy || (tail === undefined && !failed));

  const save = (event) => {
    event.preventDefault();
    if (key.trim()) put(key.trim(), 'save');
    else error = '넣을 키를 먼저 붙여 넣어 주세요.';
  };
</script>

<Shell title="설정" current="/options">
  <div class="flex flex-wrap content-start items-start gap-4">
    <section data-feature="set-gemini-key" aria-busy={tail === undefined && !failed} class="box-border min-w-0 flex-[1_1_300px] rounded-xl border border-gray-300 bg-white p-4">
      <h2 class="m-0 text-xl font-bold">Gemini API 키</h2>
      <div class="mt-2 flex items-center justify-between gap-2">
        <span>저장된 키 (끝 네 글자)</span>
        {#if failed && tail === undefined}
          <span class="text-gray-500">알 수 없음</span>
        {:else if tail === undefined}
          <span class="inline-block h-4 w-18 rounded bg-gray-300" aria-hidden="true"></span>
        {:else if tail}
          <span class="font-mono">••••{tail}</span>
        {:else}
          <span class="text-gray-500">없음</span>
        {/if}
      </div>

      {#if failed}
        <div role="alert" class="mt-4 rounded-lg bg-red-50 px-4 py-2 text-sm">
          <p class="m-0">저장된 키를 읽지 못했습니다. {reachError()}</p>
          <button type="button" onclick={load} class="mt-2 min-h-12 rounded-lg border border-gray-500 bg-white px-4">다시 읽기</button>
        </div>
      {:else if tail === null}
        <p class="m-0 mt-4 rounded-lg bg-gray-200 px-4 py-2 text-sm">저장된 키가 없습니다. 키를 넣어야 번역을 켤 수 있습니다.</p>
      {/if}

      <form onsubmit={save}>
        <label class="mt-4 block text-sm text-gray-500">
          {tail ? '바꿀 새 키' : '새 키'}
          <input
            type="text"
            data-feature="set-gemini-key"
            autocomplete="off"
            autocapitalize="off"
            spellcheck="false"
            placeholder="발급받은 키를 붙여 넣으세요"
            bind:value={key}
            disabled={locked}
            aria-invalid={!!error}
            aria-describedby="key-help"
            class={[
              'mt-1 box-border block min-h-12 w-full rounded-lg border bg-surface px-4 py-2 text-base text-ink disabled:bg-gray-100 disabled:opacity-60',
              error ? 'border-red-600' : 'border-gray-400',
            ]}
          />
        </label>
        <p id="key-help" class={['m-0 mt-1 text-sm', error ? 'text-red-700' : 'text-gray-500']}>
          {error || saved || 'Google AI Studio에서 발급받은 키를 넣습니다.'}
        </p>
        <div class="mt-4 flex flex-wrap gap-2">
          <button
            type="submit"
            data-feature="set-gemini-key"
            disabled={locked}
            class="min-h-12 flex-[2_1_160px] rounded-lg bg-primary font-semibold text-white disabled:bg-gray-400"
          >
            {busy === 'save' ? '저장하는 중' : '저장'}
          </button>
          <button
            type="button"
            data-feature="set-gemini-key"
            disabled={!!busy || !tail}
            onclick={() => put('', 'delete')}
            class="min-h-12 flex-[1_1_120px] rounded-lg border border-gray-500 bg-gray-200 text-ink disabled:text-gray-400"
          >
            {busy === 'delete' ? '지우는 중' : '키 지우기'}
          </button>
        </div>
      </form>
      <p class="m-0 mt-4 rounded-lg bg-gray-200 px-4 py-2 text-sm">키는 이 기기에만 암호화해 저장하고, Gemini 요청 말고는 기기 밖으로 나가지 않습니다.</p>
    </section>

    <section class="box-border min-w-0 flex-[1_1_300px] rounded-xl border border-gray-300 bg-white p-4">
      <h2 class="m-0 text-xl font-bold">번역 방식</h2>
      <dl class="m-0 mt-4 flex flex-col gap-2">
        {#each fixed as [name, value, note] (name)}
          <div class="flex flex-wrap items-center justify-between gap-x-2">
            <dt>{name}</dt>
            <dd class="m-0 text-gray-600">{value}</dd>
            {#if note}<p class="m-0 w-full text-sm text-gray-500">{note}</p>{/if}
          </div>
        {/each}
      </dl>
      <p class="m-0 mt-4 text-sm text-gray-500">이 항목은 고정이라 바꿀 수 없습니다.</p>
    </section>
  </div>
</Shell>
