<script lang="ts">
  import * as Dialog from "@lobbylocker/ui/components/dialog";
  import { Button } from "@lobbylocker/ui/components/button";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import type { Backend } from "$lib/backend.svelte";
  import lockupDark from "$lib/assets/brand/lobbylocker-lockup-on-dark.svg?url";
  import lockupLight from "$lib/assets/brand/lobbylocker-lockup-on-light.svg?url";

  let {
    app,
    version,
    dark,
    open = $bindable(false),
  }: {
    app: Backend;
    version: string;
    dark: boolean;
    open?: boolean;
  } = $props();
  let linkError = $state("");

  async function visit(url: string) {
    linkError = "";
    try {
      await openUrl(url);
    } catch {
      linkError = app.t("Could not open this link.");
    }
  }
</script>

<Dialog.Root bind:open>
  <Dialog.Content class="sm:max-w-md" onOpenAutoFocus={() => (linkError = "")}>
    <Dialog.Header class="items-center text-center sm:text-center">
      <img
        src={dark ? lockupDark : lockupLight}
        alt=""
        class="mb-4 h-14 w-auto"
      />
      <!-- The lockup already spells the name; keep it for screen readers. -->
      <Dialog.Title class="sr-only"
        ><span dir="ltr">LobbyLocker</span></Dialog.Title
      >
      <Dialog.Description>
        {app.t("A game server region blocker for Windows and Linux.")}
      </Dialog.Description>
    </Dialog.Header>

    <dl
      class="grid grid-cols-[auto_1fr] gap-x-5 gap-y-2 rounded-lg border p-4 text-sm"
    >
      <dt class="text-muted-foreground">{app.t("Version")}</dt>
      <dd class="text-end font-medium" dir="ltr">{version}</dd>
      <dt class="text-muted-foreground">{app.t("Created by")}</dt>
      <dd class="text-end font-medium" dir="ltr">its4lion</dd>
      <dt class="text-muted-foreground">{app.t("Platforms")}</dt>
      <dd class="text-end">{app.t("Windows and Linux")}</dd>
      <dt class="text-muted-foreground">{app.t("License")}</dt>
      <dd class="text-end" dir="ltr">MIT</dd>
    </dl>

    <div class="grid gap-2 sm:grid-cols-2">
      <Button
        variant="outline"
        onclick={() => visit("https://github.com/its4lion")}
      >
        GitHub
      </Button>
      <Button variant="outline" onclick={() => visit("https://x.com/its4lion")}>
        Twitter / X
      </Button>
    </div>

    <div
      class="flex items-center gap-3 rounded-lg bg-muted/50 px-4 py-3 text-sm"
    >
      <span class="text-muted-foreground">Discord</span>
      <span class="ms-auto font-medium" dir="ltr">@its4lion</span>
    </div>

    {#if linkError}<p class="text-center text-xs text-destructive" role="alert">
        {linkError}
      </p>{/if}

    <p class="text-center text-xs leading-relaxed text-muted-foreground">
      {app.t(
        "LobbyLocker is open source and does not modify game files or inject code into games.",
      )}
    </p>
  </Dialog.Content>
</Dialog.Root>
