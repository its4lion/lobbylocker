<script lang="ts">
  import { Button } from "@lobbylocker/ui/components/button";
  import { Input } from "@lobbylocker/ui/components/input";
  import { Checkbox } from "@lobbylocker/ui/components/checkbox";
  import { ScanLine, Search, FolderOpen, File } from "@lucide/svelte";
  import { toggleKeys } from "$lib/game";
  import GameArtwork from "./GameArtwork.svelte";
  import GameBlockBadge from "./GameBlockBadge.svelte";
  import type { Backend } from "$lib/backend.svelte";
  let {
    app,
    ondone,
    oncancel,
    onfolder,
    onexecutable,
  }: {
    app: Backend;
    ondone: () => void;
    oncancel: () => void;
    onfolder: () => void;
    onexecutable: () => void;
  } = $props();
  let chosen = $state<string[]>([]);
  let initialized = false;
  let query = $state("");
  $effect(() => {
    if (!initialized && app.status) {
      chosen = [...app.status.snapshot.preferences.libraryGameIds];
      initialized = true;
    }
  });
  const choices = $derived(
    app.candidates.filter((game) =>
      game.name.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()),
    ),
  );
  async function save() {
    if (await app.saveLibraryGames(chosen)) ondone();
  }
</script>

<section aria-label={app.t("Manage games")}>
  <div class="mb-6 flex flex-wrap items-center justify-between gap-4">
    <div>
      <h1>
        {app.t(
          app.status?.snapshot.preferences.libraryGameIds.length
            ? "Manage games"
            : "Choose games",
        )}
      </h1>
      <p class="mt-2 text-[13px] text-muted-foreground">
        {app.t("Choose the games you want in your sidebar.")}
      </p>
    </div>
    <Button variant="outline" disabled={app.busy} onclick={() => app.scan()}
      ><ScanLine />{app.t(
        app.scanning ? "Detecting games…" : "Scan games",
      )}</Button
    >
  </div>
  <div class="mb-6 flex flex-wrap items-center gap-3">
    <div class="relative min-w-48 max-w-lg flex-1">
      <Search
        class="pointer-events-none absolute start-3 top-2.5 size-4 text-muted-foreground"
      /><Input
        class="ps-9"
        aria-label={app.t("Search detected games")}
        placeholder={app.t("Search games…")}
        bind:value={query}
      />
    </div>
    <Button variant="outline" disabled={app.busy} onclick={onfolder}
      ><FolderOpen />{app.t("Add library folder")}</Button
    ><Button variant="outline" disabled={app.busy} onclick={onexecutable}
      ><File />{app.t("Choose executable")}</Button
    >
  </div>
  <div class="grid gap-3 lg:grid-cols-2">
    {#each choices as game (game.id)}<div
        class="flex items-center gap-3 rounded-lg border p-3"
        data-game-choice={game.id}
      >
        <Checkbox
          id={`choose-${game.id}`}
          checked={chosen.includes(game.id)}
          disabled={app.busy}
          onCheckedChange={(checked) =>
            (chosen = toggleKeys(chosen, [game.id], checked))}
        />
        <label
          for={`choose-${game.id}`}
          class="mb-0 flex min-w-0 flex-1 cursor-pointer items-center gap-3"
          ><GameArtwork {app} id={game.id} /><span class="min-w-0 flex-1"
            ><span
              class="block truncate text-[13px] font-medium"
              dir="auto"
              title={game.name}>{game.name}</span
            ><span
              class="mt-0.5 flex flex-wrap items-center gap-x-2 gap-y-1 text-[11px] text-muted-foreground"
              ><span>{game.launcher ?? app.t(game.availability)}</span
              ><GameBlockBadge {app} id={game.id} />{#if !game.configured}<span
                  >{app.t("Add server IPs after choosing this game.")}</span
                >{/if}</span
            ></span
          ></label
        >
      </div>{:else}<p class="empty lg:col-span-2">
        {app.t(
          app.scanning
            ? "Detecting games…"
            : query.trim()
              ? "No games match your search."
              : "No games found. Add a library folder or choose an executable.",
        )}
      </p>{/each}
  </div>
  <p class="mt-6 text-xs text-muted-foreground">
    {app.t(
      "Games with active or saved blocks stay visible. Hiding a game does not change firewall rules.",
    )}
  </p>
  <div
    class="sticky bottom-0 mt-6 flex flex-wrap justify-end gap-3 border-t py-4"
  >
    <Button variant="outline" disabled={app.busy} onclick={oncancel}
      >{app.t("Cancel")}</Button
    ><Button disabled={app.busy || !app.status} onclick={save}
      >{app.t("Show selected games")}</Button
    >
  </div>
</section>
