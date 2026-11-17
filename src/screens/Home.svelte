<script>
  import { onMount } from 'svelte';
  import Shell from '../lib/Shell.svelte';
  import { api } from '../lib/api.js';
  import { link } from '../lib/router.svelte.js';
  import { allowed, captureUnsupported, modeText, notices, permissionText, reachError, startError } from '../lib/text.js';

  // S1: turn translation on and off, and see at a glance whether the key,
  // the permissions and the capture are all right.
  let status = $state(null);
  let failed = $state(false);
  let busy = $state(null);
  let problem = $state(null);
  let asking = $state(false);
  let denied = $state(false);

  async function refresh() {
    try {
      status = await api.status();
      failed = false;
    } catch {
      failed = true;
    }
  }

  onMount(() => {
    refresh();
    const timer = setInterval(refresh, 2000);
    return () => clearInterval(timer);
  });

  let granted = $derived(!!status && status.capture_allowed && status.overlay_allowed);
  // A browser cannot capture other apps' screens: no permission changes that.
  let unsupported = $derived(status?.capture_supported === false);
  let notice = $derived(status?.notice ? notices[status.notice] : null);

  async function askPermissions() {
    asking = true;
    try {
      const p = await api.requestPermissions();
      denied = !(p.capture_allowed && p.overlay_allowed);
      return !denied;
    } catch {
      problem = { text: reachError() };
      return false;
    } finally {
      asking = false;
      await refresh();
    }
  }

  async function toggle() {
    if (!status || busy) return;
    problem = null;
    if (status.on) {
      busy = 'off';
      try {
        await api.stop();
      } catch {
        problem = { text: reachError() };
      }
      busy = null;
      await refresh();
      return;
    }
    busy = 'on';
    try {
      // R29: the first time, the permissions are asked for right here.
      if (!status.key_set) {
        problem = { text: startError('key_missing'), options: true };
      } else if (unsupported) {
        problem = { text: startError('capture_unsupported') };
      } else if (!granted && !(await askPermissions())) {
        problem = problem ?? { text: startError('permission_required') };
      } else {
        await api.start();
      }
    } catch (e) {
      if (e.code !== 'already_on') problem = { text: startError(e.code), options: e.code === 'key_missing' };
    } finally {
      busy = null;
      await refresh();
    }
  }
</script>

{#snippet bar()}
  <button
    type="button"
    data-feature="toggle-translation"
    disabled={!status || !!busy}
    onclick={toggle}
    class={[
      'h-[72px] w-full rounded-2xl text-xl font-bold',
      !status || busy ? 'bg-gray-400 text-white' : status.on ? 'border-2 border-primary bg-white text-primary' : 'bg-primary text-white',
    ]}
  >
    {#if busy === 'on'}켜는 중{:else if busy === 'off'}끄는 중{:else if status?.on}번역 끄기{:else}번역 켜기{/if}
  </button>
{/snippet}

<!-- A value not read yet: a grey bar while loading, a dash once the read failed. -->
{#snippet blank(width)}
  {#if failed}<span class="text-gray-500">—</span>{:else}<span class={['inline-block h-4 rounded bg-gray-300', width]} aria-hidden="true"></span>{/if}
{/snippet}

<Shell title="실시간 화면 인식 번역" note="일본 만화 → 한국어" current="/" {bar}>
  <div class="flex flex-wrap content-start items-start gap-4">
    <section
      data-feature="translation-status"
      aria-busy={!status && !failed}
      class="box-border min-w-0 flex-[1_1_300px] rounded-xl border border-gray-300 bg-white p-4"
    >
      <div class="flex items-center gap-2">
        <span
          class={[
            'inline-block size-4 shrink-0 rounded-full',
            status ? (status.on ? 'bg-accent' : 'bg-gray-400') : 'border-2 border-gray-400',
            !status && !failed && 'animate-pulse',
          ]}
          aria-hidden="true"
        ></span>
        <span class="text-title font-bold">
          {#if status}{status.on ? '번역 켜짐' : '번역 꺼짐'}{:else if failed}상태를 알 수 없음{:else}상태 확인 중{/if}
        </span>
      </div>
      <p class="m-0 mt-1 text-sm text-gray-500">
        {#if status?.on}
          보던 앱으로 돌아가 만화를 읽으면 말풍선 위에 번역이 뜹니다. 끄면 오버레이가 사라집니다.
        {:else if status}
          번역 켜기를 누르면 보던 앱 위에 번역과 켜고 끄는 버튼이 뜹니다.
        {:else if failed}
          앱과 다시 이어지면 지금 상태를 보여 드립니다.
        {:else}
          켜져 있는지 읽어 오는 중입니다.
        {/if}
      </p>

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
          <p class="m-0">번역을 켜려면 Gemini API 키를 먼저 넣어 주세요.</p>
          <a href="/options" onclick={link} class="mt-2 inline-flex min-h-12 items-center rounded-lg bg-primary px-4 text-white no-underline">설정으로 가기</a>
        {:else if unsupported}
          <p class="m-0">{captureUnsupported}</p>
        {:else}
          <p class="m-0">키 확인, 캡처를 막아 둔 앱, Gemini 연결에 문제가 생기면 여기에 알려 드립니다.</p>
        {/if}
      </div>

      <dl class="m-0 mt-4 flex flex-col gap-2">
        <div class="flex items-center justify-between gap-2">
          <dt>Gemini API 키</dt>
          <dd class="m-0 text-right">{#if status}{status.key_set ? `끝 ${status.key_tail}` : '없음'}{:else}{@render blank('w-18')}{/if}</dd>
        </div>
        <div class="flex items-center justify-between gap-2">
          <dt>권한</dt>
          <dd class="m-0 text-right">{#if status}{permissionText(status)}{:else}{@render blank('w-18')}{/if}</dd>
        </div>
        <div class="flex items-center justify-between gap-2">
          <dt>지금 방식</dt>
          <dd class="m-0 text-right whitespace-nowrap">{#if status}{modeText(status)}{:else}{@render blank('w-18')}{/if}</dd>
        </div>
        <div class="flex items-center justify-between gap-2">
          <dt>켠 뒤 Gemini 요청</dt>
          <dd class="m-0 text-right">{#if status}{status.gemini_requests}번{:else}{@render blank('w-12')}{/if}</dd>
        </div>
      </dl>
    </section>

    <!-- Missing permissions come first, so 권한 허용하기 shows on a phone without scrolling. -->
    <section
      data-feature="request-permissions"
      class={[
        'box-border min-w-0 flex-[1_1_300px] rounded-xl border border-gray-300 bg-white',
        status?.on && granted ? 'px-4 py-2' : 'p-4',
        status && !granted && 'order-first',
      ]}
    >
      {#if status?.on && granted}
        <details>
          <summary class="min-h-12 cursor-pointer content-center text-base font-bold">권한 모두 허용됨</summary>
          <p class="m-0 mt-2">화면 캡처와 다른 앱 위에 표시가 모두 허용되어 있습니다.</p>
        </details>
      {:else}
        <h2 class="m-0 text-xl font-bold">권한 허용</h2>
        <p class="m-0 mt-2">화면을 읽고 번역을 그 위에 그리려면 두 가지 권한이 필요합니다.</p>
        <dl class="m-0 mt-4 flex flex-col gap-2">
          <div class="flex items-center justify-between gap-2">
            <dt>화면 캡처</dt>
            <dd class="m-0">{#if status}{allowed(status.capture_allowed)}{:else}{@render blank('w-18')}{/if}</dd>
          </div>
          <div class="flex items-center justify-between gap-2">
            <dt>다른 앱 위에 표시</dt>
            <dd class="m-0">{#if status}{allowed(status.overlay_allowed)}{:else}{@render blank('w-18')}{/if}</dd>
          </div>
        </dl>
        {#if unsupported}
          <p class="m-0 mt-4 rounded-lg bg-gray-200 px-4 py-2 text-sm">이 브라우저에서는 두 권한 모두 쓸 수 없습니다. 휴대폰이나 PC에 설치한 앱에서 허용해 주세요.</p>
        {:else if denied}
          <p class="m-0 mt-4 rounded-lg bg-red-50 px-4 py-2 text-sm">
            허용하지 않으셨습니다. 화면 캡처가 있어야 보던 만화의 대사를 읽고, 다른 앱 위에 표시가 있어야 그 위에 번역을 그릴 수 있습니다.
          </p>
        {/if}
        <button
          type="button"
          data-feature="request-permissions"
          disabled={!status || asking || granted || unsupported}
          onclick={askPermissions}
          class="mt-4 min-h-12 w-full rounded-lg border border-gray-500 bg-gray-200 font-semibold text-ink disabled:text-gray-500"
        >
          {#if asking}묻는 중{:else if granted}모두 허용됨{:else if unsupported}이 브라우저에서는 쓸 수 없음{:else}권한 허용하기{/if}
        </button>
        <p class="m-0 mt-2 text-sm text-gray-500">허용하지 않으면 번역이 켜지지 않습니다. PC에서는 묻지 않습니다.</p>
      {/if}
    </section>
  </div>
</Shell>
