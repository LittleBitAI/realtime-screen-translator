<script>
  import { route, screenFor } from './lib/router.svelte.js';
  import Home from './screens/Home.svelte';
  import Library from './screens/Library.svelte';
  import Options from './screens/Options.svelte';

  const screens = { S1: Home, S2: Library, S3: Options };
  const titles = { S1: '만화 열기', S2: '번역본', S3: '설정' };
  let id = $derived(screenFor(route.path));
  let Screen = $derived(screens[id]);

  $effect(() => {
    document.title = titles[id];
  });
</script>

{#key id}
  <Screen />
{/key}
