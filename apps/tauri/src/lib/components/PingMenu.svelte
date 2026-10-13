<script lang="ts">
  import * as DropdownMenu from "@lobbylocker/ui/components/dropdown-menu";
  import * as Dialog from "@lobbylocker/ui/components/dialog";
  import { Button, buttonVariants } from "@lobbylocker/ui/components/button";
  import { ChoiceSelect } from "@lobbylocker/ui/components/choice-select";
  import { Activity, ChevronDown } from "@lucide/svelte";
  import { pingRegions, serverPingSample } from "$lib/ping";
  import type { Backend } from "$lib/backend.svelte";
  let { app, compact = false }: { app: Backend; compact?: boolean } = $props();
  let open = $state(false);
  let regionId = $state("");
  let checking = $state(false);
  const regions = $derived(pingRegions(app.game));
  const visible = $derived(pingRegions(app.game, regionId));
  $effect(() => {
    app.selected;
    open = false;
    regionId = "";
  });
  function chooseOne() {
    regionId ||= regions[0]?.id ?? "";
    open = true;
  }
  async function measure(all = false) {
    if (app.busy || (!all && !regionId)) return;
    checking = true;
    try {
      await app.checkPing(all ? undefined : regionId);
    } finally {
      checking = false;
    }
  }
  function checkAll() {
    open = false;
    void measure(true);
  }
</script>

<DropdownMenu.Root dir={app.locale?.direction ?? "ltr"}>
  <DropdownMenu.Trigger
    class={buttonVariants({
      variant: "outline",
      size: compact ? "sm" : "default",
    })}
    disabled={app.busy || !regions.length}
  >
    <Activity class={checking ? "animate-pulse" : undefined} />
    {app.t(checking ? "Checking ping…" : "Check ping")}
    <ChevronDown class="size-3" />
  </DropdownMenu.Trigger>
  <DropdownMenu.Content class="w-auto min-w-44">
    <DropdownMenu.Item onSelect={checkAll}
      >{app.t("Check all servers")}</DropdownMenu.Item
    >
    <DropdownMenu.Item onSelect={chooseOne}
      >{app.t("Check one server")}</DropdownMenu.Item
    >
  </DropdownMenu.Content>
</DropdownMenu.Root>

<Dialog.Root bind:open>
  <Dialog.Content class="sm:max-w-lg max-h-[85vh] overflow-y-auto">
    <Dialog.Header>
      <Dialog.Title>{app.t("Check server ping")}</Dialog.Title>
      <Dialog.Description>
        {app.t(
          "Choose a server to check its ping. Ping checks never change blocking selections or firewall rules.",
        )}
      </Dialog.Description>
    </Dialog.Header>
    <div class="mt-2 space-y-3">
      <ChoiceSelect
        class="w-full"
        ariaLabel={app.t("Server location")}
        placeholder={app.t("Choose a server")}
        bind:value={regionId}
        disabled={app.busy}
        options={regions.map((region) => ({
          value: region.id,
          label: `${app.t(region.area)} · ${region.name}`,
        }))}
      />
      {#if app.game && visible.length}<div
          class="max-h-72 divide-y overflow-y-auto rounded-lg border bg-muted/30"
        >
          {#each visible as region (region.id)}
            {@const sample = serverPingSample(app.game, region, app.status)}
            <div
              class="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-x-4 gap-y-1 p-3"
            >
              <p class="truncate font-medium" dir="auto">{region.name}</p>
              <p
                class={sample?.latencyMs != null
                  ? "font-semibold"
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
      {#if app.error}<p
          role="alert"
          class="text-sm text-destructive"
          dir="auto"
        >
          {app.t(app.error)}
        </p>{/if}
    </div>
    <Dialog.Footer>
      <Button disabled={app.busy || !regionId} onclick={() => measure()}>
        {#if checking}<Activity class="animate-pulse" />{/if}
        {app.t(checking ? "Checking ping…" : "Check ping")}
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
