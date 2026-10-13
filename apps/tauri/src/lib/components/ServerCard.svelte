<script lang="ts">
  import * as Card from "@lobbylocker/ui/components/card";
  import * as Dialog from "@lobbylocker/ui/components/dialog";
  import { Button } from "@lobbylocker/ui/components/button";
  import { Switch } from "@lobbylocker/ui/components/switch";
  import { Activity, Pencil } from "@lucide/svelte";
  import ServerDetails from "./ServerDetails.svelte";
  import TargetForm from "./TargetForm.svelte";
  import {
    regionSelection,
    regionBlockDescription,
    blockNote,
    toggleKeys,
  } from "$lib/game";
  import { pingDescription, serverPingSample } from "$lib/ping";
  import type { Backend } from "$lib/backend.svelte";
  import type { Game, Region } from "$lib/types";
  let {
    app,
    game,
    region,
    compact = false,
    editing = false,
  }: {
    app: Backend;
    game: Game;
    region: Region;
    compact?: boolean;
    editing?: boolean;
  } = $props();
  let open = $state(false);
  const selection = $derived(
    regionSelection(game, region, app.status?.snapshot.preferences),
  );
  const sample = $derived(serverPingSample(game, region, app.status));
  $effect(() => {
    region.id;
    app.selected;
    open = false;
  });
</script>

<Card.Root
  class={compact
    ? "server-card gap-2 py-3 [--card-spacing:--spacing(3)]"
    : "server-card gap-2 py-4"}
  data-server={selection.key}
>
  <Card.Header class="flex items-center justify-between gap-3">
    <div class="min-w-0 flex-1">
      <Card.Title class={compact ? "text-[13px]" : "text-sm"}>
        <span dir="auto" class="block truncate" title={region.name}
          >{region.name}</span
        >
      </Card.Title>
      <div
        class="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-muted-foreground"
      >
        {#if !editing}<span
            title={app.t(blockNote(app.status, [selection.key]))}
          >
            {app.t(
              regionBlockDescription(app.status, game, region) ||
                "Status unavailable",
            )}
          </span>{/if}
        {#if sample}<span
            class="inline-flex items-center gap-1 tabular-nums"
            title={sample.error ?? undefined}
            aria-live="polite"
            data-ping-results
          >
            <Activity class="size-3" />
            <span
              class={sample.latencyMs != null
                ? "font-medium text-foreground"
                : undefined}
              dir={sample.latencyMs != null ? "ltr" : undefined}
            >
              {app.t(pingDescription(sample))}
            </span>
          </span>{/if}
      </div>
    </div>
    <div class="flex shrink-0 items-center gap-2">
      {#if editing}<Button
          variant="outline"
          size="sm"
          aria-label={app.t("Edit {server}", { server: region.name })}
          aria-haspopup="dialog"
          disabled={app.busy}
          onclick={() => (open = true)}><Pencil />{app.t("Edit")}</Button
        >{:else}<Switch
          dir="ltr"
          aria-label={`${app.t("Block")} ${region.name}`}
          disabled={app.busy || !selection.available}
          checked={selection.checked}
          onCheckedChange={(checked) =>
            app.preferences((preferences) => {
              preferences.blockedRegions = toggleKeys(
                preferences.blockedRegions,
                [selection.key],
                checked,
              );
            })}
        />{/if}
    </div>
  </Card.Header>
</Card.Root>

{#if editing}
  <Dialog.Root bind:open>
    <Dialog.Content class="max-h-[85vh] overflow-y-auto sm:max-w-xl">
      <Dialog.Header>
        <Dialog.Title
          >{app.t("Edit {server}", { server: region.name })}</Dialog.Title
        >
        <Dialog.Description
          >{app.t(
            "Edit this server's addresses and ping target. Saving does not change firewall rules.",
          )}</Dialog.Description
        >
      </Dialog.Header>
      {#if app.error}<p
          role="alert"
          class="text-sm text-destructive"
          dir="auto"
        >
          {app.t(app.error)}
        </p>{/if}
      <ServerDetails {app} {game} {region} />
      <div class="flex justify-end border-t pt-3">
        <TargetForm {app} {game} area={region.area} regionId={region.id} />
      </div>
    </Dialog.Content>
  </Dialog.Root>
{/if}
