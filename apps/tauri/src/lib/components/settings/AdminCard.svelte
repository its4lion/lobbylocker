<script lang="ts">
  import * as Card from "@lobbylocker/ui/components/card";
  import * as AlertDialog from "@lobbylocker/ui/components/alert-dialog";
  import { Button } from "@lobbylocker/ui/components/button";
  import type { Backend } from "$lib/backend.svelte";
  let { app }: { app: Backend } = $props();
  let adminOpen = $state(false);

  async function restartAsAdmin() {
    adminOpen = false;
    await app.command("restart_as_admin");
  }
</script>

<Card.Root>
  <Card.Header>
    <Card.Title>{app.t("Administrator session")}</Card.Title>
    <Card.Description>
      {app.t(
        "Optionally restart LobbyLocker with administrator privileges for this session.",
      )}
    </Card.Description>
  </Card.Header>
  <Card.Content class="space-y-3">
    <div class="flex flex-wrap justify-end gap-3">
      <Button
        variant="outline"
        disabled={app.busy}
        onclick={() => (adminOpen = true)}
        >{app.t("Restart as administrator")}</Button
      >
    </div>
    <p class="text-xs leading-relaxed text-muted-foreground">
      {app.t("LobbyLocker stays elevated until you close it.")}
    </p>
  </Card.Content>
</Card.Root>

<AlertDialog.Root bind:open={adminOpen}>
  <AlertDialog.Content>
    <AlertDialog.Header>
      <AlertDialog.Title>{app.t("Restart as administrator?")}</AlertDialog.Title
      >
      <AlertDialog.Description>
        {app.t(
          "Windows will show a User Account Control prompt. The web interface, server-data requests, file dialogs, and firewall operations will all run with administrator privileges for this session.",
        )}
      </AlertDialog.Description>
    </AlertDialog.Header>
    <AlertDialog.Footer>
      <AlertDialog.Cancel>{app.t("Cancel")}</AlertDialog.Cancel>
      <AlertDialog.Action disabled={app.busy} onclick={restartAsAdmin}
        >{app.t("Restart")}</AlertDialog.Action
      >
    </AlertDialog.Footer>
  </AlertDialog.Content>
</AlertDialog.Root>
