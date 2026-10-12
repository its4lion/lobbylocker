<script lang="ts">
  import * as Card from "@lobbylocker/ui/components/card";
  import { Button } from "@lobbylocker/ui/components/button";
  import type { Backend } from "$lib/backend.svelte";
  let { app }: { app: Backend } = $props();
</script>

<Card.Root>
  <Card.Header>
    <Card.Title>{app.t("Updates")}</Card.Title>
    <Card.Description>
      {app.t("Updates are signed and always need your confirmation.")}
    </Card.Description>
  </Card.Header>
  <Card.Content class="space-y-3">
    {#if app.updateVersion}<p class="text-sm">
        {app.t("update_available", { version: app.updateVersion })}
      </p>
      {#if app.updateNotes}<p
          class="text-xs whitespace-pre-line text-muted-foreground"
          dir="auto"
        >
          {app.updateNotes}
        </p>{/if}
      <div class="flex flex-wrap justify-end gap-3">
        <Button
          disabled={app.busy || app.updateInstalling}
          onclick={() => app.installUpdate()}
          >{app.t(
            app.updateInstalling ? "Installing update…" : "Install update",
          )}</Button
        >
      </div>
    {:else}<p class="text-sm text-muted-foreground">
        {app.t(
          app.updateChecking
            ? "Checking for updates…"
            : app.updateChecked
              ? "LobbyLocker is up to date."
              : "Updates have not been checked yet.",
        )}
      </p>
      <div class="flex flex-wrap justify-end gap-3">
        <Button
          variant="outline"
          disabled={app.busy || app.updateChecking}
          onclick={() => app.checkForUpdates()}
          >{app.t("Check for updates")}</Button
        >
      </div>{/if}
    {#if app.updateError}<p class="text-xs text-destructive" dir="auto">
        {app.updateError}
      </p>{/if}
  </Card.Content>
</Card.Root>
