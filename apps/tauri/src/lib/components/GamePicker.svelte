<script lang="ts">
  import * as DropdownMenu from "@lobbylocker/ui/components/dropdown-menu";
  import { buttonVariants } from "@lobbylocker/ui/components/button";
  import { ChevronDown, Gamepad2, Settings2 } from "@lucide/svelte";
  import GameArtwork from "./GameArtwork.svelte";
  import GameBlockBadge from "./GameBlockBadge.svelte";
  import type { Backend } from "$lib/backend.svelte";
  let {
    app,
    onmanage,
    onselect,
  }: {
    app: Backend;
    onmanage: () => void;
    onselect: (id: string) => void;
  } = $props();
</script>

<div class="shrink-0 border-b px-4 py-2.5">
  <DropdownMenu.Root dir={app.locale?.direction ?? "ltr"}>
    <DropdownMenu.Trigger
      class={`${buttonVariants({ variant: "outline" })} w-full min-w-0 justify-start gap-2`}
      aria-label={app.t("Select a game")}
      disabled={app.busy || !app.status}
    >
      {#if app.game}<GameArtwork {app} id={app.game.id} small />{:else}<Gamepad2
          class="size-4 text-muted-foreground"
        />{/if}
      <span class="min-w-0 flex-1 truncate text-start" dir="auto"
        >{app.game?.name ?? app.t("Select a game")}</span
      >
      {#if app.game}<GameBlockBadge {app} id={app.game.id} />{/if}
      <ChevronDown class="size-3.5 text-muted-foreground" />
    </DropdownMenu.Trigger>
    <DropdownMenu.Content>
      <DropdownMenu.RadioGroup value={app.selected} onValueChange={onselect}>
        {#each app.games as game (game.id)}
          <DropdownMenu.RadioItem
            value={game.id}
            disabled={app.busy}
            closeOnSelect
            class="gap-2 py-1.5"
          >
            <GameArtwork {app} id={game.id} small />
            <span class="min-w-0 flex-1 truncate" dir="auto">{game.name}</span>
            <GameBlockBadge {app} id={game.id} />
          </DropdownMenu.RadioItem>
        {/each}
      </DropdownMenu.RadioGroup>
      {#if app.games.length}<DropdownMenu.Separator />{/if}
      <DropdownMenu.Item
        onSelect={onmanage}
        disabled={app.busy}
        class="gap-2 py-1.5"
      >
        <Settings2 class="size-4 text-muted-foreground" />
        {app.t("Manage games")}
      </DropdownMenu.Item>
    </DropdownMenu.Content>
  </DropdownMenu.Root>
</div>
