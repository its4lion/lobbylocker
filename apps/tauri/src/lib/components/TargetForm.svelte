<script lang="ts">
  import { Button } from "@lobbylocker/ui/components/button";
  import { Input } from "@lobbylocker/ui/components/input";
  import { Label } from "@lobbylocker/ui/components/label";
  import { ChoiceSelect } from "@lobbylocker/ui/components/choice-select";
  import * as Dialog from "@lobbylocker/ui/components/dialog";
  import { Plus } from "@lucide/svelte";
  import type { Backend } from "$lib/backend.svelte";
  import type { Endpoint, Game, Protocol } from "$lib/types";
  let {
    app,
    game,
    area,
    regionId,
  }: { app: Backend; game: Game; area: string; regionId?: string } = $props();
  const prefix = $props.id();
  let address = $state("");
  let protocol = $state<Protocol>("udp");
  let ports = $state("");
  let attachment = $state("__default");
  let location = $state("");
  let open = $state(false);
  const regions = $derived(game.regions.filter((r) => r.area === area));
  async function save(event: SubmitEvent) {
    event.preventDefault();
    const updated = structuredClone($state.snapshot(game));
    const endpoint: Endpoint = {
      id: `custom-${crypto.randomUUID()}`,
      address: address.trim(),
      protocol,
      ports: ports.trim() || null,
      custom: true,
    };
    if (regionId) {
      // The caller already scoped this form to one server location.
      const scoped = updated.regions.find(
        (r) => r.area === area && r.id === regionId,
      );
      if (!scoped) return;
      scoped.endpoints.push(endpoint);
    } else if (attachment === "__new" || !regions.length) {
      updated.regions.push({
        id: `custom-${crypto.randomUUID()}`,
        name: location.trim() || app.t("My servers"),
        area,
        probeTarget: null,
        endpoints: [endpoint],
      });
    } else {
      const region = updated.regions.find(
        (r) =>
          r.area === area &&
          r.id === (attachment === "__default" ? regions[0]?.id : attachment),
      );
      if (!region) return;
      region.endpoints.push(endpoint);
    }
    if (await app.saveGame(updated)) {
      address = "";
      ports = "";
      location = "";
      open = false;
    }
  }
</script>

{#snippet fields()}
  <fieldset disabled={app.busy} class="space-y-4">
    <div class="grid gap-2">
      <Label for={`${prefix}-address`}>{app.t("IP address or subnet")}</Label
      ><Input
        id={`${prefix}-address`}
        dir="ltr"
        required
        bind:value={address}
        placeholder="203.0.113.8 / 2001:db8::/64"
      />
    </div>
    <div class="grid grid-cols-2 gap-3">
      <div class="grid gap-2">
        <Label for={`${prefix}-protocol`}>{app.t("Protocol")}</Label
        ><ChoiceSelect
          id={`${prefix}-protocol`}
          bind:value={protocol}
          disabled={app.busy}
          options={[
            { value: "udp", label: "UDP" },
            { value: "tcp", label: "TCP" },
            { value: "any", label: app.t("Any") },
          ]}
        />
      </div>
      <div class="grid gap-2">
        <Label for={`${prefix}-ports`}>{app.t("Remote port / range")}</Label
        ><Input
          id={`${prefix}-ports`}
          dir="ltr"
          bind:value={ports}
          placeholder={app.t("All ports")}
        />
      </div>
    </div>
    {#if !regionId}<div class="grid gap-2">
        <Label for={`${prefix}-region`}>{app.t("Server location")}</Label
        ><ChoiceSelect
          id={`${prefix}-region`}
          bind:value={attachment}
          disabled={app.busy}
          options={[
            {
              value: "__default",
              label:
                regions[0]?.name ??
                app.t("+ New server location in this region"),
            },
            ...regions.map((region) => ({
              value: region.id,
              label: region.name,
            })),
            {
              value: "__new",
              label: app.t("+ New server location in this region"),
            },
          ]}
        />
      </div>{/if}
    {#if !regionId && (attachment === "__new" || !regions.length)}<div
        class="grid gap-2"
      >
        <Label for={`${prefix}-location`}
          >{app.t("New server location name")}</Label
        ><Input id={`${prefix}-location`} bind:value={location} />
      </div>{/if}
    <Button type="submit" disabled={!address.trim()}
      ><Plus />{app.t("Save target")}</Button
    >
  </fieldset>
  <p class="mt-4 text-xs text-muted-foreground">
    {app.t(
      "Targets follow this server's toggle. Save does not send a ping or change the firewall; use Apply when ready.",
    )}
  </p>
{/snippet}

<Button size="sm" disabled={app.busy} onclick={() => (open = true)}
  ><Plus />{app.t("Add an IP")}</Button
>
<Dialog.Root bind:open>
  <Dialog.Content class="max-h-[85vh] overflow-y-auto sm:max-w-xl">
    <Dialog.Header>
      <Dialog.Title>{app.t("Add an IP or subnet")}</Dialog.Title>
      <Dialog.Description
        >{app.t("Add a custom address to this region.")}</Dialog.Description
      >
    </Dialog.Header>
    <form onsubmit={save} class="mt-4">{@render fields()}</form>
  </Dialog.Content>
</Dialog.Root>
