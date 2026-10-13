<script lang="ts">
  import { Button } from "@lobbylocker/ui/components/button";
  import { Info, Pencil, RefreshCw } from "@lucide/svelte";
  import AreaCard from "./AreaCard.svelte";
  import TargetForm from "./TargetForm.svelte";
  import GameProgram from "./GameProgram.svelte";
  import PingMenu from "./PingMenu.svelte";
  import { windowsBlocking } from "$lib/firewall";
  import { gameAreas } from "$lib/game";
  import type { Backend } from "$lib/backend.svelte";
  let {
    app,
    compact = false,
    onmanage,
  }: { app: Backend; compact?: boolean; onmanage: () => void } = $props();
  let editing = $state(false);
  const regionPanel = $props.id();
  $effect(() => {
    app.selected;
    compact;
    // The compact window has no Edit mode control, so it must never keep one.
    editing = false;
  });
</script>

{#if app.game}
  {@const game = app.game}
  {@const refreshable = ["overwatch2", "cs2", "deadlock"].includes(game.id)}
  {@const hasTargets = game.regions.some((region) => region.endpoints.length)}
  {@const hiddenRanges = game.regions.filter(
    (region) => region.providerScope,
  ).length}
  {#if !compact}<div
      class="mb-6 flex flex-wrap items-center justify-between gap-4"
    >
      <div>
        <h1 dir="auto">{game.name}</h1>
        <p class="mt-2 text-[13px] text-muted-foreground">
          {app.t("Choose which servers to block, individually or by region.")}
        </p>
      </div>
      <PingMenu {app} />
    </div>{/if}
  {#if windowsBlocking(app.status)}<GameProgram {app} {game} />{/if}
  <div
    class={compact
      ? "mb-3 flex flex-wrap items-center justify-between gap-2"
      : "mb-4 flex flex-wrap items-center justify-between gap-3"}
  >
    <div>
      <div class="relative flex items-center gap-2">
        <h2 class="font-medium">{app.t("Block servers")}</h2>
        <span class="h-5 w-px bg-border" aria-hidden="true"></span>
        <button
          type="button"
          class="group inline-flex size-8 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          aria-label={app.t("Server data information")}
        >
          <Info class="size-4" />
          <span
            role="tooltip"
            class="pointer-events-none absolute start-0 top-full z-50 mt-2 hidden w-80 max-w-[calc(100vw-3rem)] rounded-lg border bg-popover p-3 text-start text-xs leading-relaxed text-popover-foreground shadow-lg group-hover:block group-focus-within:block"
            dir="auto"
          >
            <strong class="mb-1 block text-sm font-medium"
              >{app.t("Server data information")}</strong
            >
            {game.note}
          </span>
        </button>
      </div>
    </div>
    <div class="flex flex-wrap items-center gap-2">
      {#if compact}<PingMenu {app} {compact} />{/if}
      {#if refreshable && (!compact || !hasTargets)}<Button
          variant={hasTargets ? "ghost" : "default"}
          size="sm"
          disabled={app.busy}
          onclick={() => app.command("refresh_game", { gameId: game.id })}
          ><RefreshCw />{app.t(
            !hasTargets
              ? "Get server data"
              : game.id === "overwatch2"
                ? "Refresh cloud ranges"
                : "Refresh official list",
          )}</Button
        >{/if}
      {#if !compact}<Button
          variant={editing ? "secondary" : "outline"}
          size="sm"
          aria-pressed={editing}
          aria-controls={regionPanel}
          disabled={!game.regions.length}
          onclick={() => (editing = !editing)}
          ><Pencil />{app.t(editing ? "Done editing" : "Edit mode")}</Button
        >{/if}
    </div>
  </div>
  {#if hiddenRanges && !compact}
    <p class="mb-4 text-xs text-muted-foreground">
      {app.t(
        "Some regions are hosted on shared cloud ranges. Blocking one can affect other applications.",
      )}
    </p>
  {/if}
  <div
    id={regionPanel}
    class={compact ? "space-y-5" : "space-y-6"}
    data-view={editing ? "edit" : "basic"}
  >
    {#each gameAreas(game) as area (`${game.id}:${area}`)}<AreaCard
        {app}
        {game}
        {area}
        {compact}
        editing={editing && !compact}
      />{/each}
  </div>
  {#if !game.regions.length}<div>
      <p class="mx-auto mb-4 max-w-2xl text-sm text-muted-foreground">
        {app.t(
          "Add your first server IP. Saving selections does not change existing firewall rules; use Apply when ready.",
        )}
      </p>
      <div class="flex justify-center">
        <TargetForm {app} {game} area="Custom" />
      </div>
    </div>{/if}
{:else}<div class="flex min-h-80 items-center justify-center text-center">
    <div>
      <h1>
        {app.t(
          app.scanning
            ? "Detecting games…"
            : app.candidates.length
              ? "Choose games"
              : "Your game library is empty",
        )}
      </h1>
      <p class="mt-3 text-sm text-muted-foreground">
        {app.t(
          app.scanning
            ? "Checking local launcher folders."
            : "Choose which games appear in your sidebar.",
        )}
      </p>
      <Button
        variant="outline"
        class="mt-5"
        disabled={app.busy}
        onclick={onmanage}>{app.t("Choose games")}</Button
      >
    </div>
  </div>{/if}
