<script lang="ts">
  import * as Card from "@lobbylocker/ui/components/card";
  import { Button } from "@lobbylocker/ui/components/button";
  import type { Backend } from "$lib/backend.svelte";
  let { app }: { app: Backend } = $props();
</script>

<Card.Root>
  <Card.Header>
    <Card.Title>{app.t("Local game files")}</Card.Title>
    <Card.Description>
      {app.t("Your games are stored as JSON files and never overwritten.")}
    </Card.Description>
  </Card.Header>
  <Card.Content class="space-y-4">
    <code>{app.status?.snapshot.configDir}</code>
    <div class="flex flex-wrap justify-end gap-3">
      <Button
        variant="outline"
        disabled={app.busy}
        onclick={() => app.importGame()}>{app.t("Import game JSON")}</Button
      ><Button
        variant="outline"
        disabled={app.busy || !app.game}
        onclick={() => app.command("export_game", { gameId: app.selected })}
        >{app.t("Export selected game")}</Button
      >
    </div>
  </Card.Content>
</Card.Root>
