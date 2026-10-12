<script lang="ts">
  import { onMount } from "svelte";
  import packageInfo from "../package.json";
  import { Backend } from "$lib/backend.svelte";
  import { actionBar, applyReview } from "$lib/action-bar";
  import { scopeWarning, missingPrograms } from "$lib/firewall";
  import * as Tabs from "@lobbylocker/ui/components/tabs";
  import * as AlertDialog from "@lobbylocker/ui/components/alert-dialog";
  import * as Dialog from "@lobbylocker/ui/components/dialog";
  import { Button } from "@lobbylocker/ui/components/button";
  import { ChoiceSelect } from "@lobbylocker/ui/components/choice-select";
  import { Gamepad2, Moon, Sun } from "@lucide/svelte";
  import GameLibrary from "$lib/components/GameLibrary.svelte";
  import ManageGames from "$lib/components/ManageGames.svelte";
  import CustomGame from "$lib/components/CustomGame.svelte";
  import Regions from "$lib/components/Regions.svelte";
  import Connections from "$lib/components/Connections.svelte";
  import Settings from "$lib/components/Settings.svelte";
  import AboutDialog from "$lib/components/AboutDialog.svelte";
  const app = new Backend();
  const version = packageInfo.version;
  let page = $state("Regions");
  let view = $state<"main" | "custom">("main");
  let customPath = $state("");
  let content = $state<HTMLDivElement | null>(null);
  let confirmOpen = $state(false);
  let manageOpen = $state(false);
  let aboutOpen = $state(false);
  let dark = $state(true);
  const pages = ["Regions", "Connections", "Settings"];
  const direction = $derived(app.locale?.direction ?? "ltr");
  const bar = $derived(actionBar(app.status, app.busy));
  const review = $derived(applyReview(app.status));
  const missing = $derived(missingPrograms(app.status));
  const feedback = $derived(
    app.firewallAction
      ? app.firewallSlow
        ? "Still waiting for administrator approval or the firewall helper. No new request has been sent."
        : app.firewallAction === "apply"
          ? "Applying… Check the administrator prompt."
          : app.firewallAction === "read"
            ? "Reading firewall… Administrator access may be requested. No rules will be changed."
            : "Resetting… Check the administrator prompt."
      : app.firewallFeedback === "apply"
        ? "Firewall changes applied."
        : app.firewallFeedback === "reset"
          ? "LobbyLocker blocks removed."
          : app.firewallFeedback === "error"
            ? "Firewall operation not confirmed. Review the error above."
            : app.busy
              ? "Working…"
              : bar.apply
                ? review.regions
                  ? app.status?.verification === "different"
                    ? "apply_review_different"
                    : "apply_review"
                  : "apply_review_empty"
                : bar.key,
  );
  onMount(() => {
    void app.initialize().then(() => {
      if (app.status && !app.games.length) manageOpen = true;
    });
  });
  $effect(() => {
    document.documentElement.dir = direction;
    document.documentElement.lang = app.locale?.code ?? "en";
    document.documentElement.classList.toggle("dark", dark);
  });
  $effect(() => {
    view;
    page;
    app.selected;
    content?.scrollTo({ top: 0 });
  });
  // Opening a game page refreshes its official server data in the background,
  // so the user never needs the refresh button.
  $effect(() => {
    const id = app.selected;
    if (view === "main" && page === "Regions" && id)
      void app.ensureFreshData(id);
  });
  function manage() {
    app.error = "";
    manageOpen = true;
  }
  async function addFolder() {
    if (await app.addLibrary()) manage();
  }
  async function addExecutable() {
    const path = await app.pickExecutable();
    if (path) {
      manageOpen = false;
      customPath = path;
      view = "custom";
    }
  }
  function showGame(id?: string) {
    if (id) app.selected = id;
    view = "main";
    page = "Regions";
    app.error = "";
  }
  async function reset() {
    confirmOpen = false;
    if (!(await app.command("reset_rules"))) content?.scrollTo({ top: 0 });
  }
</script>

<Tabs.Root
  bind:value={page}
  dir={direction}
  class="grid h-dvh grid-rows-[auto_minmax(0,1fr)] overflow-hidden"
  onValueChange={() => (view = "main")}
>
  <header
    class="flex flex-wrap items-center justify-between gap-4 border-b px-6 py-4"
  >
    <div class="flex items-center gap-2 font-semibold">
      <Gamepad2 class="size-5" /><span dir="ltr">LobbyLocker</span>
    </div>
    <Tabs.List aria-label={app.t("Main navigation")} class="h-auto flex-wrap"
      >{#each pages as item}<Tabs.Trigger
          value={item}
          onclick={() => (view = "main")}>{app.t(item)}</Tabs.Trigger
        >{/each}</Tabs.List
    >
    <div class="flex items-center gap-2">
      <ChoiceSelect
        class="w-32"
        ariaLabel={app.t("Language")}
        placeholder={app.t("Language")}
        disabled={app.busy || !app.status}
        value={app.code}
        options={app.languages.locales.map((locale) => ({
          value: locale.code,
          label: locale.name,
        }))}
        onValueChange={(code) => app.command("set_language", { code })}
      /><Button
        variant="ghost"
        size="icon"
        aria-label={app.t("Toggle theme")}
        onclick={() => (dark = !dark)}
        >{#if dark}<Sun />{:else}<Moon />{/if}</Button
      >
    </div>
  </header>
  <div
    class="grid min-h-0 grid-cols-[minmax(0,1fr)_285px] overflow-hidden [direction:ltr] max-[1000px]:grid-cols-[minmax(0,1fr)_250px] max-[759px]:grid-cols-1 max-[759px]:grid-rows-[minmax(0,3fr)_minmax(0,2fr)]"
  >
    <main class="flex min-h-0 min-w-0 flex-col overflow-hidden" dir={direction}>
      <!-- svelte-ignore a11y_no_noninteractive_tabindex (Keyboard users must be able to focus and scroll this region.) -->
      <div
        class="mx-auto w-full max-w-[1400px] flex-1 min-h-0 overflow-auto overscroll-contain p-7 max-[1000px]:p-5"
        bind:this={content}
        aria-busy={app.busy}
        role="region"
        aria-label={app.t("Main content")}
        tabindex="0"
      >
        {#if app.error}<div
            role="alert"
            class="mb-5 rounded-lg border border-destructive p-4 text-sm text-destructive"
            dir="auto"
          >
            {app.t(app.error)}
          </div>{/if}
        {#if app.firewallError}<div
            role="alert"
            data-firewall-error
            class="mb-5 rounded-lg border border-destructive p-4 text-sm"
          >
            <p class="text-destructive">
              {app.t(
                "Could not verify firewall status. Retry to check which blocks are active.",
              )}
            </p>
            <details class="mt-2 text-xs text-muted-foreground">
              <summary>{app.t("Error details")}</summary>
              <p class="mt-2" dir="auto">{app.firewallError}</p>
            </details>
            <Button
              class="mt-3"
              size="sm"
              variant="outline"
              disabled={app.busy}
              onclick={() => app.readFirewall()}>{app.t("Retry")}</Button
            >
          </div>{/if}
        {#if app.status}
          {#if view === "custom"}{#key customPath}<CustomGame
                {app}
                path={customPath}
                ondone={() => showGame()}
                oncancel={() => (view = "main")}
              />{/key}
          {:else}
            <Tabs.Content value="Regions"
              ><Regions {app} onmanage={manage} /></Tabs.Content
            >
            <Tabs.Content value="Connections"
              ><Connections {app} /></Tabs.Content
            >
            <Tabs.Content value="Settings"
              ><Settings
                {app}
                onreset={() => (confirmOpen = true)}
                onmanage={manage}
              /></Tabs.Content
            >
          {/if}
        {:else if app.busy}<p
            class="p-5 py-10 text-center text-muted-foreground"
            role="status"
          >
            {app.t("Working…")}
          </p>{:else}<Button variant="outline" onclick={() => app.initialize()}
            >{app.t("Retry")}</Button
          >{/if}
      </div>
      {#if bar.visible || app.firewallAction || app.firewallFeedback}<footer
          class="flex shrink-0 flex-wrap items-center justify-between gap-4 border-t bg-background px-7 py-3"
        >
          <div class="min-w-0 flex-1">
            <p
              class="text-xs text-muted-foreground"
              role={app.firewallFeedback === "error" ? "alert" : "status"}
              title={review.games.join(", ")}
            >
              {app.t(feedback, {
                ...bar.values,
                games: review.games.length,
                regions: review.regions,
              })}
            </p>
            {#if bar.apply || app.firewallAction === "apply"}<p
                class="mt-1 max-w-2xl text-xs text-muted-foreground"
              >
                {app.t(scopeWarning(app.status))}
              </p>{/if}
            {#if bar.apply && missing.length}<p
                class="mt-1 text-xs text-destructive"
              >
                {app.t("Choose executables for these games before blocking")}:
                <span dir="auto"
                  >{missing.map((target) => target.gameName).join(", ")}</span
                >
              </p>{/if}
          </div>
        </footer>{/if}
    </main>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex (Keyboard users must be able to focus and scroll this region.) -->
    <div
      class="min-h-0 min-w-0 overflow-auto overscroll-contain"
      dir={direction}
      role="region"
      aria-label={app.t("Game library")}
      tabindex="0"
    >
      <GameLibrary
        {app}
        {version}
        managing={manageOpen}
        onmanage={manage}
        onselect={showGame}
        onabout={() => (aboutOpen = true)}
      />
    </div>
  </div>
</Tabs.Root>
<Dialog.Root bind:open={manageOpen}>
  <Dialog.Content
    class="sm:max-w-3xl max-h-[85vh] overflow-y-auto"
    aria-describedby={undefined}
  >
    <Dialog.Title class="sr-only">{app.t("Manage games")}</Dialog.Title>
    <ManageGames
      {app}
      ondone={() => (manageOpen = false)}
      oncancel={() => (manageOpen = false)}
      onfolder={addFolder}
      onexecutable={addExecutable}
    />
  </Dialog.Content>
</Dialog.Root>
<AlertDialog.Root bind:open={confirmOpen}>
  <AlertDialog.Content
    ><AlertDialog.Header
      ><AlertDialog.Title>{app.t("Reset")}</AlertDialog.Title
      ><AlertDialog.Description
        >{app.t(
          "Remove only LobbyLocker-owned firewall rules? Other firewall rules are untouched.",
        )}<span class="mt-2 block text-xs"
          >{app.t(
            "Deleting an AppImage cannot clean up. Reset first, wait for confirmation, then delete it.",
          )}</span
        ></AlertDialog.Description
      ></AlertDialog.Header
    ><AlertDialog.Footer
      ><AlertDialog.Cancel>{app.t("Cancel")}</AlertDialog.Cancel
      ><AlertDialog.Action disabled={app.busy} onclick={reset}
        >{app.t("Reset")}</AlertDialog.Action
      ></AlertDialog.Footer
    ></AlertDialog.Content
  >
</AlertDialog.Root>
<AboutDialog {app} {version} bind:open={aboutOpen} />
