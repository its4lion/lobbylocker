<script lang="ts">
  import { Button, buttonVariants } from "@lobbylocker/ui/components/button";
  import { Input } from "@lobbylocker/ui/components/input";
  import { Search, Plus, Gamepad2 } from "@lucide/svelte";
  import GameArtwork from "./GameArtwork.svelte";
  import GameBlockBadge from "./GameBlockBadge.svelte";
  import type { Backend } from "$lib/backend.svelte";
  let {
    app,
    version,
    managing = false,
    onmanage,
    onselect,
    onabout,
  }: {
    app: Backend;
    version: string;
    managing?: boolean;
    onmanage: () => void;
    onselect: (id: string) => void;
    onabout: () => void;
  } = $props();
  let query = $state("");
  const games = $derived(app.games);
  const visible = $derived(
    games.filter((game) =>
      game.name.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()),
    ),
  );
</script>

<aside
  class="flex min-h-full min-w-0 flex-col border-s px-4 py-6 max-[759px]:border-s-0 max-[759px]:border-t"
  aria-label={app.t("Game library")}
>
  <h2 class="font-semibold">{app.t("Games")}</h2>
  <div class="mt-5 flex gap-2">
    <div class="relative min-w-0 flex-1">
      <Search
        class="pointer-events-none absolute start-3 top-2.5 size-4 text-muted-foreground"
      /><Input
        class="ps-9"
        aria-label={app.t("Search games")}
        placeholder={app.t("Search games…")}
        bind:value={query}
      />
    </div>
    <Button
      class={buttonVariants({ variant: "outline", size: "icon" })}
      disabled={app.busy || !app.status || managing}
      aria-label={app.t("Manage games")}
      onclick={onmanage}><Plus class="size-4" /></Button
    >
  </div>
  {#if games.length}
    <div class="mt-3 flex-1 space-y-1">
      {#each visible as game (game.id)}
        <button
          type="button"
          class="flex w-full items-center gap-3 rounded-md p-3 text-start text-muted-foreground hover:bg-secondary hover:text-foreground data-[active=true]:bg-secondary data-[active=true]:text-foreground"
          data-active={game.id === app.selected}
          disabled={app.busy}
          aria-pressed={game.id === app.selected}
          onclick={() => onselect(game.id)}
        >
          <GameArtwork {app} id={game.id} />
          <span class="min-w-0 flex-1"
            ><span class="block truncate font-medium" dir="auto"
              >{game.name}</span
            ><span class="mt-1 flex flex-wrap items-center gap-1.5"
              ><GameBlockBadge {app} id={game.id} /></span
            ></span
          >
        </button>
      {:else}<p class="p-4 text-sm text-muted-foreground">
          {app.t("No games match your search.")}
        </p>{/each}
    </div>
  {:else}
    <div
      class="flex min-h-64 flex-1 flex-col items-center justify-center gap-4 py-10 text-center"
    >
      <Gamepad2 class="size-8 text-muted-foreground" />
      <p class="max-w-48 text-sm text-muted-foreground">
        {app.t(
          app.scanning ? "Detecting games…" : "Choose which games appear here.",
        )}
      </p>
      <Button
        variant="outline"
        disabled={app.busy || !app.status || managing}
        onclick={onmanage}>{app.t("Choose games")}</Button
      >
    </div>
  {/if}
  <button
    type="button"
    class="mt-4 flex w-full flex-col items-start gap-[3px] border-t p-3 text-start text-[11px] text-muted-foreground transition-colors hover:text-foreground focus-visible:text-foreground"
    onclick={onabout}
  >
    <span class="font-medium" dir="ltr">LobbyLocker v{version}</span>
    <span>{app.t("Created by")} <b dir="ltr">its4lion</b></span>
  </button>
</aside>
