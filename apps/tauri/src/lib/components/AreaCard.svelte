<script lang="ts">
  import * as Collapsible from "@lobbylocker/ui/components/collapsible";
  import { Switch } from "@lobbylocker/ui/components/switch";
  import { ChevronDown } from "@lucide/svelte";
  import ServerCard from "./ServerCard.svelte";
  import TargetForm from "./TargetForm.svelte";
  import { areaSelection, selectionDescription, toggleKeys } from "$lib/game";
  import type { Backend } from "$lib/backend.svelte";
  import type { Game } from "$lib/types";
  let {
    app,
    game,
    area,
    compact = false,
    editing = false,
  }: {
    app: Backend;
    game: Game;
    area: string;
    compact?: boolean;
    editing?: boolean;
  } = $props();
  let open = $state(true);
  const prefix = $props.id();
  const selection = $derived(
    app.status
      ? areaSelection(game, area, app.status.snapshot.preferences)
      : { keys: [], selected: 0, checked: false },
  );
  const regions = $derived(game.regions.filter((r) => r.area === area));
</script>

<!-- One region group in both states: a bulk toggle, a collapsible heading, and
     the individual server cards. Edit mode adds the per-server Edit buttons and
     an Add IP button on the heading. -->
<Collapsible.Root
  bind:open
  class="space-y-3"
  aria-labelledby={`${prefix}-area`}
  data-server-area={area}
>
  <div class="space-y-1">
    <div class="flex items-center gap-3">
      <h2 class="min-w-0 flex-1 text-sm font-medium">
        <Collapsible.Trigger
          class="flex w-full items-center gap-2 rounded-md py-1 text-start outline-none focus-visible:ring-2 focus-visible:ring-ring"
          aria-label={app.t(open ? "Collapse {region}" : "Expand {region}", {
            region: app.t(area),
          })}
        >
          <ChevronDown
            class={`size-3.5 shrink-0 text-muted-foreground transition-transform ${open ? "" : "-rotate-90 rtl:rotate-90"}`}
          />
          <span id={`${prefix}-area`} class="shrink-0">{app.t(area)}</span>
          <span class="ms-1 h-px min-w-4 flex-1 bg-border" aria-hidden="true"
          ></span>
        </Collapsible.Trigger>
      </h2>
      {#if editing}<TargetForm {app} {game} {area} />{:else}<Switch
          dir="ltr"
          disabled={app.busy || !selection.keys.length}
          aria-label={app.t("Block all servers in {region}", {
            region: app.t(area),
          })}
          aria-describedby={selection.selected > 0 && !selection.checked
            ? `${prefix}-selection`
            : undefined}
          checked={selection.checked}
          onCheckedChange={(checked) =>
            app.preferences((preferences) => {
              preferences.blockedRegions = toggleKeys(
                preferences.blockedRegions,
                selection.keys,
                checked,
              );
            })}
        />{/if}
    </div>
    {#if !editing && selection.selected > 0 && !selection.checked}<p
        id={`${prefix}-selection`}
        class="text-[11px] text-muted-foreground"
      >
        {app.t(selectionDescription(selection.checked, selection.selected))}
      </p>{/if}
  </div>
  <Collapsible.Content
    class={compact
      ? "grid grid-cols-[minmax(0,1fr)] items-start gap-2"
      : "grid grid-cols-[repeat(auto-fill,minmax(230px,1fr))] items-start gap-3"}
  >
    {#each regions as region (region.id)}<ServerCard
        {app}
        {game}
        {region}
        {compact}
        {editing}
      />{/each}
  </Collapsible.Content>
</Collapsible.Root>
