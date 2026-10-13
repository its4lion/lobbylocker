<script lang="ts">
  import { Shield } from "@lucide/svelte";
  import { gameBlockState, blockNote } from "$lib/game";
  import type { Backend } from "$lib/backend.svelte";
  let { app, id }: { app: Backend; id: string } = $props();
  const state = $derived(gameBlockState(id, app.status));
  const keys = $derived(
    Object.keys(app.status?.regionStates ?? {}).filter((key) =>
      key.startsWith(`${id}:`),
    ),
  );
</script>

{#if state === "blocked"}<span
    data-block-state={state}
    class="inline-flex shrink-0 items-center gap-1 whitespace-nowrap rounded-md bg-amber-500/10 px-1.5 py-0.5 text-[11px] text-amber-600 dark:text-amber-400"
    title={app.t(blockNote(app.status, keys))}
    ><Shield class="size-3" />{app.t("Blocked")}</span
  >{/if}
