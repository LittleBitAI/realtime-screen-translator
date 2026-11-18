<script>
  import { link } from './router.svelte.js';

  let { title, current, children } = $props();

  const tabs = [
    { path: '/', label: '만화 열기' },
    { path: '/library', label: '번역본' },
    { path: '/options', label: '설정' },
  ];
</script>

<div class="flex min-h-dvh flex-col bg-surface">
  <a href="#screen-content" class="sr-only z-20 rounded-lg bg-white p-3 focus:not-sr-only focus:absolute focus:start-4 focus:top-2">
    본문으로 건너뛰기
  </a>
  <div class="workspace-nav sticky z-10 border-gray-300 bg-white">
    <nav aria-label="화면" class="workspace-width flex gap-2">
      {#each tabs as tab (tab.path)}
        <a
          href={tab.path}
          onclick={link}
          aria-current={current === tab.path ? 'page' : undefined}
          class={[
            'flex min-h-12 min-w-0 flex-1 items-center justify-center rounded-lg px-2 text-center text-sm font-semibold no-underline min-[62rem]:flex-none min-[62rem]:px-4',
            current === tab.path ? 'bg-selection text-primary underline decoration-2 underline-offset-4' : 'text-muted hover:bg-gray-100',
          ]}>{tab.label}</a
        >
      {/each}
    </nav>
  </div>
  <header class="workspace-width pt-6 pb-4 min-[62rem]:pt-8 min-[62rem]:pb-6">
    <h1 class="m-0 text-title leading-[1.2] font-bold">{title}</h1>
  </header>
  <main id="screen-content" tabindex="-1" class="workspace-width flex-1 pb-6 min-[62rem]:scroll-mt-20 min-[62rem]:pb-8">
    {@render children()}
  </main>
</div>

<style>
  .workspace-width {
    width: 100%;
    max-width: 90rem;
    margin-inline: auto;
    padding-inline: 1rem;
  }

  .workspace-nav {
    order: 1;
    bottom: 0;
    border-top-width: 1px;
    padding-block: .5rem calc(.5rem + env(safe-area-inset-bottom));
  }

  @media (min-width: 62rem) {
    .workspace-width { padding-inline: 2rem; }
    .workspace-nav {
      order: 0;
      top: 0;
      bottom: auto;
      border-top-width: 0;
      border-bottom-width: 1px;
      padding-block: .5rem;
    }
  }
</style>
