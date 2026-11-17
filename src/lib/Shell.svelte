<script>
  import { link } from './router.svelte.js';

  /** One screen: its header, its body, and the bar at the thumb's reach. */
  let { title, note = '', current, children, bar } = $props();

  const tabs = [
    { path: '/', label: '번역' },
    { path: '/library', label: '번역본' },
    { path: '/options', label: '설정' },
  ];
</script>

<div class="mx-auto flex min-h-screen max-w-[960px] flex-col bg-surface">
  <header class="flex items-center justify-between gap-2 p-4">
    <h1 class="m-0 text-xl font-bold">{title}</h1>
    {#if note}<span class="shrink-0 text-sm text-gray-500">{note}</span>{/if}
  </header>
  <main class="flex-1 px-4 pb-4">
    {@render children()}
  </main>
  <div class="sticky bottom-0 border-t border-gray-300 bg-surface px-4 pt-2 pb-2">
    {#if bar}{@render bar()}{/if}
    <nav aria-label="화면" class="mt-2 flex gap-2">
      {#each tabs as tab (tab.path)}
        <a
          href={tab.path}
          onclick={link}
          aria-current={current === tab.path ? 'page' : undefined}
          class={[
            'flex min-h-12 flex-1 items-center justify-center rounded-lg no-underline',
            current === tab.path ? 'bg-gray-300 font-bold text-ink' : 'text-gray-600 hover:bg-gray-200',
          ]}>{tab.label}</a
        >
      {/each}
    </nav>
  </div>
</div>
