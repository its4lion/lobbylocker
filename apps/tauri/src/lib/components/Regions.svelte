<script lang="ts">
  import { Button } from "@lobbylocker/ui/components/button";
  import { ChoiceSelect } from "@lobbylocker/ui/components/choice-select";
  import * as Dialog from "@lobbylocker/ui/components/dialog";
  import { Activity, Info, RefreshCw, SlidersHorizontal } from "@lucide/svelte";
  import AreaCard from "./AreaCard.svelte";
  import TargetForm from "./TargetForm.svelte";
  import GameProgram from "./GameProgram.svelte";
  import { windowsBlocking } from "$lib/firewall";
  import { gameAreas } from "$lib/game";
  import type { Backend } from "$lib/backend.svelte";
  let { app, onmanage }: { app: Backend; onmanage: () => void } = $props();
  let advanced = $state(false);
  let checking = $state(false);
  let pingOpen = $state(false);
  let pingArea = $state("");
  const regionPanel = $props.id();
  const pingRegions = $derived(
    (app.game?.regions ?? []).filter((region) => region.probeTarget),
  );
  const pingAreas = $derived(
    [...new Set(pingRegions.map((region) => region.area))].sort(),
  );
  const selectedPingRegions = $derived(
    pingRegions.filter((region) => region.area === pingArea),
  );
  $effect(() => {
    app.selected;
    advanced = false;
    pingOpen = false;
    pingArea = "";
  });
  function openPing() {
    pingArea ||= pingAreas[0] ?? "";
    pingOpen = true;
  }
  async function checkSelected(gameId: string) {
    if (!pingArea) return;
    checking = true;
    try {
      await app.command("check_ping", { gameId, area: pingArea });
    } finally {
      checking = false;
    }
  }
</script>

{#if app.game}
  {@const game = app.game}
  {@const refreshable = ["overwatch2", "cs2", "deadlock"].includes(game.id)}
  {@const hasTargets = game.regions.some((region) => region.endpoints.length)}
  <div class="mb-6 flex flex-wrap items-center justify-between gap-4">
    <div>
      <h1 dir="auto">{game.name}</h1>
      <p class="mt-2 text-[13px] text-muted-foreground">
        {app.t(
          "Choose which regions to block. Changes take effect after Apply.",
        )}
      </p>
    </div>
    <Button
      variant="outline"
      disabled={app.busy || !pingRegions.length}
      onclick={openPing}><Activity />{app.t("Check ping")}</Button
    >
  </div>
  {#if windowsBlocking(app.status)}<GameProgram {app} {game} />{/if}
  <div class="mb-4 flex flex-wrap items-center justify-between gap-3">
    <div>
      <div class="flex items-center gap-2">
        <h2 class="font-medium">{app.t("Block regions")}</h2>
        <span class="h-5 w-px bg-border" aria-hidden="true"></span>
        <button
          type="button"
          class="group relative inline-flex size-8 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          aria-label={app.t("Server data information")}
        >
          <Info class="size-4" />
          <span
            role="tooltip"
            class="pointer-events-none absolute start-0 top-full z-50 mt-2 hidden w-80 rounded-lg border bg-popover p-3 text-start text-xs leading-relaxed text-popover-foreground shadow-lg group-hover:block group-focus-within:block"
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
      {#if refreshable}<Button
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
      <Button
        variant={advanced ? "secondary" : "outline"}
        size="sm"
        aria-expanded={advanced}
        aria-controls={regionPanel}
        disabled={!game.regions.length}
        onclick={() => (advanced = !advanced)}
        ><SlidersHorizontal />{app.t(
          advanced ? "Basic view" : "Advanced",
        )}</Button
      >
    </div>
  </div>
  <Dialog.Root bind:open={pingOpen}>
    <Dialog.Content class="sm:max-w-lg">
      <Dialog.Header>
        <Dialog.Title>{app.t("Check server ping")}</Dialog.Title>
        <Dialog.Description
          >{app.t(
            "Choose a region to test every configured server location in it. Ping checks never change blocking selections or firewall rules.",
          )}</Dialog.Description
        >
      </Dialog.Header>
      <div class="mt-4 space-y-4">
        <ChoiceSelect
          class="w-full"
          ariaLabel={app.t("Region")}
          placeholder={app.t("Choose a region")}
          bind:value={pingArea}
          disabled={app.busy}
          options={pingAreas.map((area) => ({
            value: area,
            label: app.t(area),
          }))}
        />
        {#if selectedPingRegions.length}<div
            class="max-h-72 divide-y overflow-y-auto rounded-lg border bg-muted/30"
          >
            {#each selectedPingRegions as region (region.id)}
              {@const sample = app.status?.samples[`${game.id}:${region.id}`]}
              <div
                class="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-x-4 gap-y-1 p-3"
              >
                <p class="truncate font-medium" dir="auto">{region.name}</p>
                <p
                  class={sample?.latencyMs != null
                    ? "font-semibold text-foreground"
                    : "text-muted-foreground"}
                >
                  {sample
                    ? sample.latencyMs == null
                      ? app.t("No reply")
                      : `${Math.round(sample.latencyMs)} ms`
                    : app.t("Not checked")}
                </p>
                <code class="truncate text-xs text-muted-foreground"
                  >{region.probeTarget}</code
                >
                {#if sample?.error}<span
                    class="max-w-44 truncate text-end text-xs text-muted-foreground"
                    title={sample.error}>{sample.error}</span
                  >{/if}
              </div>
            {/each}
          </div>{/if}
      </div>
      <Dialog.Footer>
        <Button
          disabled={app.busy || !pingArea}
          onclick={() => checkSelected(game.id)}
          >{#if checking}<Activity class="animate-pulse" />{/if}{app.t(
            checking ? "Checking ping…" : "Check ping",
          )}</Button
        >
      </Dialog.Footer>
    </Dialog.Content>
  </Dialog.Root>
  <div
    id={regionPanel}
    class="grid items-start gap-4 grid-cols-[repeat(auto-fill,minmax(230px,1fr))]"
    class:grid-cols-[minmax(0,1fr)]={advanced}
    data-view={advanced ? "advanced" : "basic"}
  >
    {#each gameAreas(game) as area (`${game.id}:${area}`)}<AreaCard
        {app}
        {game}
        {area}
        {advanced}
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
