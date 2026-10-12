<script lang="ts">
  import * as Card from "@lobbylocker/ui/components/card";
  import { Button } from "@lobbylocker/ui/components/button";
  import type { Backend } from "$lib/backend.svelte";
  let { app, onreset }: { app: Backend; onreset: () => void } = $props();
</script>

<Card.Root>
  <Card.Header>
    <Card.Title>{app.t("Firewall recovery")}</Card.Title>
    <Card.Description>
      {app.t("Removes the firewall rules LobbyLocker added.")}
    </Card.Description>
  </Card.Header>
  <Card.Content>
    <div class="flex flex-wrap justify-end gap-3">
      <Button variant="destructive" disabled={app.busy} onclick={onreset}
        >{app.t("Reset")}</Button
      >
    </div>
  </Card.Content>
</Card.Root>
