<script lang="ts">
  import type { Backend } from "$lib/backend.svelte";
  import GameFilesCard from "$lib/components/settings/GameFilesCard.svelte";
  import LibraryFoldersCard from "$lib/components/settings/LibraryFoldersCard.svelte";
  import FirewallRecoveryCard from "$lib/components/settings/FirewallRecoveryCard.svelte";
  import AdminCard from "$lib/components/settings/AdminCard.svelte";
  import UpdatesCard from "$lib/components/settings/UpdatesCard.svelte";
  let {
    app,
    onreset,
    onmanage,
  }: { app: Backend; onreset: () => void; onmanage: () => void } = $props();
</script>

<div class="mb-6 flex flex-wrap items-center justify-between gap-4">
  <div>
    <h1>{app.t("Settings")}</h1>
    <p class="mt-2 text-[13px] text-muted-foreground">
      {app.t("Local files, game data, and firewall recovery.")}
    </p>
  </div>
</div>

<div class="grid gap-5 xl:grid-cols-2">
  <GameFilesCard {app} />
  {#if app.status?.snapshot.platform === "windows"}<AdminCard {app} />{/if}
  {#if app.status?.selfUpdateSupported}<UpdatesCard {app} />{/if}
  <LibraryFoldersCard {app} {onmanage} />
  <FirewallRecoveryCard {app} {onreset} />
</div>
