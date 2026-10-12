<script lang="ts">
  import * as Card from "@lobbylocker/ui/components/card";
  import { Button } from "@lobbylocker/ui/components/button";
  import { X } from "@lucide/svelte";
  import type { Backend } from "$lib/backend.svelte";
  let { app, onmanage }: { app: Backend; onmanage: () => void } = $props();
</script>

<Card.Root>
  <Card.Header>
    <Card.Title>{app.t("Game library folders")}</Card.Title>
    <Card.Description>
      {app.t("Removing a folder stops discovery there. No files are deleted.")}
    </Card.Description>
  </Card.Header>
  <Card.Content class="space-y-4">
    {#each app.libraryFolders as folder (folder.path)}<div
        class="flex items-center gap-3 rounded-lg border bg-muted/50 p-3"
        data-library-path={folder.path}
      >
        <div class="min-w-0 flex-1">
          <code class="block truncate" dir="ltr">{folder.path}</code>
          <p class="mt-1 text-xs text-muted-foreground">
            {app.t(folder.source)}
          </p>
        </div>
        <Button
          variant="ghost"
          size="icon-sm"
          disabled={app.busy}
          aria-label={app.t("Remove")}
          title={app.t("Remove")}
          onclick={() => app.removeLibrary(folder.path)}
          ><X class="size-4" /></Button
        >
      </div>{:else}<p class="text-sm text-muted-foreground">
        {app.t(
          app.scanning ? "Detecting games…" : "No library folders configured.",
        )}
      </p>{/each}
    <div class="flex flex-wrap justify-end gap-3">
      <Button
        variant="outline"
        disabled={app.busy}
        onclick={() => app.addLibrary()}>{app.t("Add library folder")}</Button
      >
      <Button variant="ghost" disabled={app.busy} onclick={onmanage}
        >{app.t("Manage games")}</Button
      >
    </div>
    {#if app.installed.warnings.length}<details>
        <summary>{app.t("Scan warnings")}</summary
        >{#each app.installed.warnings as warning}<p
            class="mt-2 text-xs text-muted-foreground"
            dir="auto"
          >
            {warning}
          </p>{/each}
      </details>{/if}
  </Card.Content>
</Card.Root>
