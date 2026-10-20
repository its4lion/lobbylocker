<script lang="ts">
  import * as Collapsible from "@lobbylocker/ui/components/collapsible";
  import { Button } from "@lobbylocker/ui/components/button";
  import { Input } from "@lobbylocker/ui/components/input";
  import { Label } from "@lobbylocker/ui/components/label";
  import { ChoiceSelect } from "@lobbylocker/ui/components/choice-select";
  import { Plus, FolderOpen } from "@lucide/svelte";
  import { areas } from "$lib/game";
  import { localIcon } from "$lib/artwork";
  import { executableBlocking } from "$lib/firewall";
  import type { Backend } from "$lib/backend.svelte";
  import type { Protocol } from "$lib/types";
  let {
    app,
    path,
    ondone,
    oncancel,
  }: { app: Backend; path: string; ondone: () => void; oncancel: () => void } =
    $props();
  let executablePath = $state("");
  let name = $state("");
  let address = $state("");
  let area = $state("Custom");
  let protocol = $state<Protocol>("udp");
  let ports = $state("");
  let advanced = $state(false);
  let initialized = false;
  const needsExecutable = $derived(executableBlocking(app.status));
  function basename(path: string) {
    return (
      path
        .split(/[\\/]/)
        .pop()
        ?.replace(/\.[^.]+$/, "") ?? ""
    );
  }
  $effect(() => {
    if (!initialized) {
      executablePath = path;
      name = basename(path);
      initialized = true;
    }
  });
  async function browse() {
    const chosen = await app.pickExecutable();
    if (chosen) {
      executablePath = chosen;
      if (!name.trim()) name = basename(chosen);
    }
  }
  async function save(event: SubmitEvent) {
    event.preventDefault();
    if (!name.trim() || !address.trim() || (needsExecutable && !executablePath))
      return;
    if (
      await app.create({
        candidateId: null,
        name,
        address,
        area,
        protocol,
        ports: ports.trim() || null,
        executablePath: executablePath || null,
      })
    )
      ondone();
  }
</script>

<div class="mb-6 flex flex-wrap items-center justify-between gap-4">
  <div>
    <h1>{app.t("Add custom game")}</h1>
  </div>
</div>
<form onsubmit={save} class="max-w-2xl space-y-5">
  <fieldset disabled={app.busy} class="space-y-5">
    {#if executablePath && localIcon(app.pickedIcon)}<img
        src={localIcon(app.pickedIcon)}
        alt=""
        class="size-14 rounded-lg object-contain"
      />{/if}
    {#if needsExecutable || executablePath}<div class="grid gap-2">
        <Label for="game-executable">{app.t("Game executable")}</Label>
        <div class="flex gap-2">
          <Input
            id="game-executable"
            value={executablePath}
            readonly
            required
            dir="ltr"
            class="min-w-0 flex-1"
          /><Button variant="outline" type="button" onclick={browse}
            ><FolderOpen />{app.t("Browse")}</Button
          >
        </div>
      </div>{/if}
    <div class="grid gap-2">
      <Label for="game-name">{app.t("Game name")}</Label><Input
        id="game-name"
        required
        maxlength={120}
        bind:value={name}
      />
    </div>
    <div class="grid gap-2">
      <Label for="game-address">{app.t("Server IP or subnet")}</Label><Input
        id="game-address"
        required
        dir="ltr"
        bind:value={address}
        placeholder="203.0.113.8 / 2001:db8::/64"
      />
    </div>
    <div class="grid gap-2">
      <Label for="game-area">{app.t("Region")}</Label><ChoiceSelect
        id="game-area"
        bind:value={area}
        disabled={app.busy}
        options={areas.map((value) => ({ value, label: app.t(value) }))}
      />
    </div>
    <Collapsible.Root bind:open={advanced}
      ><Collapsible.Trigger class="text-sm text-muted-foreground"
        >{app.t("Advanced · protocol and port")}</Collapsible.Trigger
      ><Collapsible.Content class="mt-3 grid grid-cols-2 gap-3">
        <div class="grid gap-2">
          <Label for="game-protocol">{app.t("Protocol")}</Label><ChoiceSelect
            id="game-protocol"
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
          <Label for="game-ports">{app.t("Port / range")}</Label><Input
            id="game-ports"
            dir="ltr"
            bind:value={ports}
            placeholder={app.t("All ports")}
          />
        </div>
      </Collapsible.Content></Collapsible.Root
    >
  </fieldset>
  <div class="flex justify-end gap-3">
    <Button
      type="button"
      variant="outline"
      disabled={app.busy}
      onclick={oncancel}>{app.t("Cancel")}</Button
    ><Button
      type="submit"
      disabled={app.busy ||
        !name.trim() ||
        !address.trim() ||
        (needsExecutable && !executablePath)}
      ><Plus />{app.t("Add game")}</Button
    >
  </div>
</form>
