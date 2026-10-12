<script lang="ts">
  import * as Card from "@lobbylocker/ui/components/card";
  import { Button } from "@lobbylocker/ui/components/button";
  import { Switch } from "@lobbylocker/ui/components/switch";
  import { Minus, Plus } from "@lucide/svelte";
  import ServerDetails from "./ServerDetails.svelte";
  import TargetForm from "./TargetForm.svelte";
  import {
    areaSelection,
    areaBlockDescription,
    blockNote,
    selectionDescription,
    toggleKeys,
  } from "$lib/game";
  import type { Backend } from "$lib/backend.svelte";
  import type { Game } from "$lib/types";
  let {
    app,
    game,
    area,
    advanced = false,
  }: {
    app: Backend;
    game: Game;
    area: string;
    advanced?: boolean;
  } = $props();
  let expanded = $state(true);
  const selection = $derived(
    app.status
      ? areaSelection(game, area, app.status.snapshot.preferences)
      : { keys: [], selected: 0, checked: false },
  );
  const regions = $derived(game.regions.filter((r) => r.area === area));
</script>

<Card.Root class="area-card">
  <Card.Header class="flex items-start justify-between gap-4"
    ><div class="min-w-0 flex-1">
      <Card.Title>{app.t(area)}</Card.Title><Card.Description
        class="mt-2"
        title={app.t(blockNote(app.status, selection.keys))}
        >{app.t(
          areaBlockDescription(app.status, selection.keys),
        )}{#if app.status?.dirty && selection.selected > 0 && !selection.checked}<span
            class="mt-1 block text-xs"
            >{app.t(
              selectionDescription(selection.checked, selection.selected),
            )}</span
          >{/if}</Card.Description
      >
    </div>
    <div class="flex shrink-0 items-center gap-2">
      <Switch
        class="mt-0.5"
        dir="ltr"
        disabled={app.busy || !selection.keys.length}
        aria-label={`${app.t("Block")} ${app.t(area)}`}
        checked={selection.checked}
        onCheckedChange={(checked) =>
          app.preferences((p) => {
            p.blockedRegions = toggleKeys(
              p.blockedRegions,
              selection.keys,
              checked,
            );
          })}
      />
      {#if advanced}<Button
          variant="ghost"
          size="icon-sm"
          aria-label={app.t(expanded ? "Collapse region" : "Expand region")}
          aria-expanded={expanded}
          onclick={() => (expanded = !expanded)}
          >{#if expanded}<Minus class="size-4" />{:else}<Plus
              class="size-4"
            />{/if}</Button
        >{/if}
    </div></Card.Header
  >
  {#if advanced && expanded}<Card.Content>
      <div class="space-y-4">
        <div class="flex items-center gap-3">
          <div class="h-px min-w-8 flex-1 bg-border"></div>
          <TargetForm {app} {game} {area} />
        </div>
        <div class="grid items-start gap-4 md:grid-cols-2 2xl:grid-cols-3">
          {#each regions as region (region.id)}<ServerDetails
              {app}
              {game}
              {region}
            />{/each}
        </div>
      </div>
    </Card.Content>
  {:else if !selection.keys.length}<Card.Content
      ><p class="text-xs text-muted-foreground">
        {app.t("Add an IP in Advanced or refresh the list to enable blocking.")}
      </p></Card.Content
    >{/if}
</Card.Root>
