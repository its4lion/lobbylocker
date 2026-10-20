<script lang="ts">
  import * as Card from "@lobbylocker/ui/components/card";
  import { Button } from "@lobbylocker/ui/components/button";
  import { ChoiceSelect } from "@lobbylocker/ui/components/choice-select";
  import { Label } from "@lobbylocker/ui/components/label";
  import { executableBlocking, missingPrograms } from "$lib/firewall";
  import type { Backend } from "$lib/backend.svelte";
  let { app }: { app: Backend } = $props();
</script>

<Card.Root class="min-w-0" data-blocking-settings>
  <Card.Header>
    <Card.Title>{app.t("Blocking")}</Card.Title>
  </Card.Header>
  <Card.Content class="space-y-4">
    <div class="flex flex-wrap items-center justify-between gap-3">
      <Label for="blocking-scope">{app.t("Apply to")}</Label>
      <ChoiceSelect
        id="blocking-scope"
        class="w-full sm:w-56"
        disabled={app.busy}
        value={app.status?.snapshot.preferences.windowsBlockingScope ??
          "executable"}
        options={[
          { value: "executable", label: app.t("Selected games") },
          { value: "systemWide", label: app.t("All apps on this PC") },
        ]}
        onValueChange={(scope) => app.setBlockingScope(scope)}
      />
    </div>
    {#if executableBlocking(app.status) && app.games.length}<details
        open={missingPrograms(app.status).length > 0}
        data-game-executables
      >
        <summary>{app.t("Game executables")}</summary>
        <div class="mt-3 divide-y">
          {#each app.games as game (game.id)}<div
              class="flex items-center gap-3 py-3 first:pt-0 last:pb-0"
              data-game-executable={game.id}
            >
              <div class="min-w-0 flex-1">
                <p
                  class="truncate text-sm font-medium"
                  dir="auto"
                  title={game.name}
                >
                  {game.name}
                </p>
                {#if game.executablePath}<p
                    class="mt-1 truncate font-mono text-xs text-muted-foreground"
                    dir="ltr"
                    title={game.executablePath}
                  >
                    {game.executablePath}
                  </p>{:else}<p class="mt-1 text-xs text-muted-foreground">
                    {app.t("Executable not found")}
                  </p>{/if}
              </div>
              <Button
                variant="outline"
                size="sm"
                disabled={app.busy}
                aria-label={app.t("Change executable for {game}", {
                  game: game.name,
                })}
                onclick={() =>
                  app.command("set_game_executable", { gameId: game.id })}
                >{app.t(game.executablePath ? "Change" : "Browse")}</Button
              >
            </div>{/each}
        </div>
      </details>{:else if !executableBlocking(app.status)}<p
        class="text-xs text-muted-foreground"
      >
        {app.t("Blocks selected servers for every app, not just games.")}
      </p>{/if}
    {#if app.status?.dirty}<div class="flex justify-end">
        <Button
          size="sm"
          disabled={app.busy || missingPrograms(app.status).length > 0}
          onclick={() => app.apply()}>{app.t("Apply changes")}</Button
        >
      </div>{/if}
  </Card.Content>
</Card.Root>
