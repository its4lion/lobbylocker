<script lang="ts">
  import { Button } from "@lobbylocker/ui/components/button";
  import { Input } from "@lobbylocker/ui/components/input";
  import { Label } from "@lobbylocker/ui/components/label";
  import { Switch } from "@lobbylocker/ui/components/switch";
  import { Activity, Minus, Plus, Trash2 } from "@lucide/svelte";
  import {
    regionKey,
    toggleKeys,
    regionBlockState,
    blockDescription,
    blockNote,
  } from "$lib/game";
  import type { Backend } from "$lib/backend.svelte";
  import type { Game, Region } from "$lib/types";
  let { app, game, region }: { app: Backend; game: Game; region: Region } =
    $props();
  const prefix = $props.id();
  let probe = $state("");
  let expanded = $state(false);
  $effect(() => {
    probe = region.probeTarget ?? "";
  });
  const key = $derived(regionKey(game, region));
  const sample = $derived(app.status?.samples[key]);
  async function saveProbe() {
    const updated = structuredClone($state.snapshot(game));
    const target = updated.regions.find((r) => r.id === region.id);
    if (target) {
      target.probeTarget = probe.trim() || null;
      await app.saveGame(updated);
    }
  }
  async function remove(id: string) {
    const updated = structuredClone($state.snapshot(game));
    const target = updated.regions.find((r) => r.id === region.id);
    if (target) {
      target.endpoints = target.endpoints.filter((e) => e.id !== id);
      await app.saveGame(updated);
    }
  }
</script>

<section class="space-y-3 rounded-lg border bg-background p-4">
  <div class="flex items-center justify-between gap-3">
    <div>
      <h3 class="font-medium" dir="auto">{region.name}</h3>
      <code class="text-xs text-muted-foreground">{region.id}</code>
      <p
        class="mt-1 text-xs text-muted-foreground"
        title={app.t(blockNote(app.status, [key]))}
      >
        {app.t(
          region.endpoints.length
            ? blockDescription(regionBlockState(app.status, key))
            : "No targets configured",
        )}
      </p>
      {#if regionBlockState(app.status, key) === "partial"}<p
          class="mt-1 text-xs text-muted-foreground"
        >
          {app.t(blockNote(app.status, [key]))}
        </p>{/if}
      {#if region.probeTarget}<div
          class="mt-2 flex flex-wrap items-center gap-1.5 text-xs text-muted-foreground"
          title={sample?.error ?? undefined}
        >
          <Activity class="size-3" />
          <code class="text-[11px]">{region.probeTarget}</code>
          <span aria-hidden="true">·</span>
          <span
            class={sample?.latencyMs != null
              ? "font-medium text-foreground"
              : ""}
            >{sample
              ? sample.latencyMs == null
                ? app.t("No reply")
                : `${Math.round(sample.latencyMs)} ms`
              : app.t("Not checked")}</span
          >
        </div>{/if}
    </div>
    <div class="flex shrink-0 items-center gap-2">
      <Switch
        dir="ltr"
        aria-label={`${app.t("Block")} ${region.name}`}
        disabled={app.busy || !region.endpoints.length}
        checked={app.status?.snapshot.preferences.blockedRegions.includes(
          key,
        ) ?? false}
        onCheckedChange={(checked) =>
          app.preferences((p) => {
            p.blockedRegions = toggleKeys(p.blockedRegions, [key], checked);
          })}
      />
      <Button
        variant="ghost"
        size="icon-sm"
        aria-label={app.t(expanded ? "Collapse server" : "Expand server")}
        aria-expanded={expanded}
        onclick={() => (expanded = !expanded)}
        >{#if expanded}<Minus class="size-4" />{:else}<Plus
            class="size-4"
          />{/if}</Button
      >
    </div>
  </div>
  {#if expanded}<div class="max-h-52 overflow-y-auto border-t pt-2">
      {#each region.endpoints as endpoint (endpoint.id)}
        <div class="flex flex-wrap items-center justify-between gap-2 py-1">
          <code>{endpoint.address}</code><span class="flex items-center gap-2"
            ><small
              >{endpoint.protocol.toUpperCase()} · {endpoint.ports ??
                app.t("All ports")}</small
            >{#if endpoint.custom}<Button
                variant="ghost"
                size="icon-sm"
                aria-label={`${app.t("Delete")} ${endpoint.address}`}
                disabled={app.busy}
                onclick={() => remove(endpoint.id)}
                ><Trash2 class="size-3" /></Button
              >{/if}</span
          >
        </div>
      {:else}<p class="text-xs text-muted-foreground">
          {app.t("No addresses yet. Add one below.")}
        </p>{/each}
    </div>
    <div class="grid gap-2 border-t pt-3">
      <Label for={`${prefix}-probe`}>{app.t("Ping target (literal IP)")}</Label>
      <div class="mt-2 flex flex-wrap gap-2">
        <Input
          id={`${prefix}-probe`}
          class="min-w-40 flex-1"
          dir="ltr"
          bind:value={probe}
          disabled={app.busy}
          placeholder={app.t("Known responding IP; leave blank to disable")}
        /><Button
          variant="outline"
          size="sm"
          disabled={app.busy}
          onclick={saveProbe}>{app.t("Save probe")}</Button
        ><Button
          variant="outline"
          size="sm"
          disabled={app.busy || !region.probeTarget}
          onclick={() =>
            app.command("check_ping", { gameId: game.id, regionId: region.id })}
          ><Activity />{app.t("Check ping")}</Button
        >
      </div>
    </div>
  {/if}
</section>
