<script lang="ts">
  import { Button } from "@lobbylocker/ui/components/button";
  import { ScanLine } from "@lucide/svelte";
  import type { Backend } from "$lib/backend.svelte";
  let { app }: { app: Backend } = $props();
</script>

<div class="mb-6 flex flex-wrap items-center justify-between gap-4">
  <div>
    <h1>{app.t("Connections")}</h1>
    <p class="mt-2 text-[13px] text-muted-foreground">
      {app.t(
        "Inspect current remote connections. No capture driver, no background scanning.",
      )}
    </p>
  </div>
  <Button variant="outline" disabled={app.busy} onclick={() => app.discover()}
    ><ScanLine />{app.t("Scan connections")}</Button
  >
</div>
<p
  class="mb-[22px] rounded-md border bg-muted p-[12px_15px] text-xs leading-[1.8] text-muted-foreground"
>
  {app.t("Connections are not verified game servers. This view is read-only.")}
</p>
<div class="overflow-x-auto rounded-lg border">
  <table class="w-full text-start text-sm">
    <thead
      ><tr class="border-b text-muted-foreground"
        >{#each ["Remote address", "Protocol", "Process", "State"] as heading}<th
            class="p-3 text-start font-medium">{app.t(heading)}</th
          >{/each}</tr
      ></thead
    ><tbody
      >{#each app.connections as connection}<tr class="border-b"
          ><td class="p-3"
            ><code>{connection.remoteIp}:{connection.remotePort}</code></td
          ><td class="p-3">{connection.protocol.toUpperCase()}</td><td
            class="p-3"
            dir="auto">{connection.process}</td
          ><td class="p-3">{connection.state}</td></tr
        >{:else}<tr
          ><td colspan="4" class="p-5 py-10 text-center text-muted-foreground"
            >{app.t(
              "Click Scan connections to inspect active connections.",
            )}</td
          ></tr
        >{/each}</tbody
    >
  </table>
</div>
