<script lang="ts">
  import { onMount, tick } from "svelte";
  import packageInfo from "../package.json";
  import { Backend } from "$lib/backend.svelte";
  import { actionBar, applyReview } from "$lib/action-bar";
  import {
    scopeWarning,
    executableBlocking,
    missingPrograms,
    verificationIssueMessage,
    verificationIssueRetryable,
  } from "$lib/firewall";
  import {
    loadLayout,
    saveLayout,
    WindowLayout,
    type AppLayout,
  } from "$lib/layout";
  import * as Tabs from "@lobbylocker/ui/components/tabs";
  import * as AlertDialog from "@lobbylocker/ui/components/alert-dialog";
  import * as Dialog from "@lobbylocker/ui/components/dialog";
  import { Button } from "@lobbylocker/ui/components/button";
  import { ChoiceSelect } from "@lobbylocker/ui/components/choice-select";
  import {
    ChevronsLeft,
    ChevronsRight,
    Moon,
    RotateCcw,
    Sun,
  } from "@lucide/svelte";
  import lockupDark from "$lib/assets/brand/lobbylocker-lockup-on-dark.svg?url";
  import lockupLight from "$lib/assets/brand/lobbylocker-lockup-on-light.svg?url";
  import GameLibrary from "$lib/components/GameLibrary.svelte";
  import GamePicker from "$lib/components/GamePicker.svelte";
  import ManageGames from "$lib/components/ManageGames.svelte";
  import CustomGame from "$lib/components/CustomGame.svelte";
  import Regions from "$lib/components/Regions.svelte";
  import Connections from "$lib/components/Connections.svelte";
  import Settings from "$lib/components/Settings.svelte";
  import AboutDialog from "$lib/components/AboutDialog.svelte";
  const app = new Backend();
  const windowLayout = new WindowLayout();
  const version = packageInfo.version;
  let page = $state("Regions");
  let view = $state<"main" | "custom">("main");
  let customPath = $state("");
  let content = $state<HTMLDivElement | null>(null);
  let confirmOpen = $state(false);
  let manageOpen = $state(false);
  let aboutOpen = $state(false);
  let updatePromptOpen = $state(false);
  let dark = $state(true);
  $effect(() => {
    document.documentElement.classList.toggle("dark", dark);
  });
  let layout = $state<AppLayout>("expanded");
  let changingLayout = $state(false);
  let layoutError = $state("");
  let layoutButton = $state<HTMLButtonElement | null>(null);
  const compact = $derived(layout === "collapsed");
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
    void changeLayout(loadLayout());
    void app.initialize().then(() => {
      if (app.status && !app.games.length) manageOpen = true;
      if (app.updateVersion) updatePromptOpen = true;
    });
  });
  function manage() {
    app.error = "";
    manageOpen = true;
  }
  async function addFolder() {
    if (await app.addLibrary()) manage();
  }
  async function addExecutable() {
    const needsExecutable = executableBlocking(app.status);
    const selection = needsExecutable
      ? await app.addExecutable()
      : { path: "", gameId: null };
    if (selection !== null) {
      if (selection.gameId) {
        manageOpen = false;
        showGame(selection.gameId);
        return;
      }
      if (compact && !(await changeLayout("expanded"))) return;
      manageOpen = false;
      customPath = selection.path;
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
  async function showSettings() {
    if (compact && !(await changeLayout("expanded"))) return;
    view = "main";
    page = "Settings";
    await tick();
    content?.scrollTo({ top: 0 });
  }
  async function changeLayout(next: AppLayout, restoreFocus = false) {
    if (changingLayout) return false;
    changingLayout = true;
    layoutError = "";
    try {
      await windowLayout.set(next);
      layout = next;
      if (next === "collapsed") {
        view = "main";
        page = "Regions";
      }
      saveLayout(next);
      return true;
    } catch (error) {
      layoutError = String(error);
      content?.scrollTo({ top: 0 });
      return false;
    } finally {
      changingLayout = false;
      if (restoreFocus) {
        await tick();
        layoutButton?.focus();
      }
    }
  }
</script>

{#snippet themeToggle()}
  <Button
    variant="ghost"
    size={compact ? "icon-sm" : "icon"}
    aria-label={app.t("Toggle theme")}
    title={app.t("Toggle theme")}
    onclick={() => (dark = !dark)}
    >{#if dark}<Sun />{:else}<Moon />{/if}</Button
  >
{/snippet}

{#snippet layoutToggle()}
  <Button
    bind:ref={layoutButton}
    variant="outline"
    size={compact ? "icon-sm" : "icon"}
    aria-label={app.t(compact ? "Expand app" : "Collapse app")}
    title={app.t(compact ? "Expand app" : "Collapse app")}
    disabled={changingLayout || view === "custom"}
    onclick={() => changeLayout(compact ? "expanded" : "collapsed", true)}
    >{#if compact}<ChevronsRight class="rtl:rotate-180" />{:else}<ChevronsLeft
        class="rtl:rotate-180"
      />{/if}</Button
  >
{/snippet}

<Tabs.Root
  bind:value={page}
  dir={direction}
  class="grid h-dvh grid-rows-[auto_minmax(0,1fr)] overflow-hidden"
  data-layout={layout}
  onValueChange={() => (view = "main")}
>
  <header
    class={compact
      ? "flex items-center justify-between gap-3 border-b px-4 py-3"
      : "flex flex-wrap items-center justify-between gap-4 border-b px-6 py-4"}
  >
    <div class="flex items-center">
      <img
        src={dark ? lockupDark : lockupLight}
        alt="LobbyLocker"
        class={compact ? "h-7 w-auto" : "h-9 w-auto"}
      />
    </div>
    {#if compact}
      {@render layoutToggle()}
    {:else}
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
        />
        {@render themeToggle()}
        {@render layoutToggle()}
      </div>
    {/if}
  </header>
  <div
    class={compact
      ? "grid min-h-0 grid-cols-1 overflow-hidden"
      : "grid min-h-0 grid-cols-[minmax(0,1fr)_285px] overflow-hidden [direction:ltr] max-[1000px]:grid-cols-[minmax(0,1fr)_250px] max-[759px]:grid-cols-1 max-[759px]:grid-rows-[minmax(0,3fr)_minmax(0,2fr)]"}
  >
    <main class="flex min-h-0 min-w-0 flex-col overflow-hidden" dir={direction}>
      {#if compact}
        <GamePicker {app} onmanage={manage} onselect={showGame} />
      {/if}
      <!-- svelte-ignore a11y_no_noninteractive_tabindex (Keyboard users must be able to focus and scroll this region.) -->
      <div
        class={compact
          ? "min-h-0 w-full flex-1 overflow-auto overscroll-contain p-4"
          : "mx-auto w-full max-w-[1400px] flex-1 min-h-0 overflow-auto overscroll-contain p-7 max-[1000px]:p-5"}
        bind:this={content}
        aria-busy={app.busy}
        role="region"
        aria-label={app.t("Main content")}
        tabindex="0"
      >
        {#if layoutError}<div
            role="alert"
            class="mb-5 rounded-lg border border-destructive p-4 text-sm text-destructive"
          >
            <p>{app.t("Could not resize the app window. Try again.")}</p>
            <details class="mt-2 text-xs">
              <summary>{app.t("Error details")}</summary>
              <p class="mt-2" dir="auto">{layoutError}</p>
            </details>
          </div>{/if}
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
                verificationIssueMessage(app.status?.verificationIssue) ??
                  "Could not verify firewall status. Retry to check which blocks are active.",
              )}
            </p>
            <details class="mt-2 text-xs text-muted-foreground">
              <summary>{app.t("Error details")}</summary>
              <p class="mt-2" dir="auto">{app.firewallError}</p>
            </details>
            {#if verificationIssueRetryable(app.status?.verificationIssue)}<Button
                class="mt-3"
                size="sm"
                variant="outline"
                disabled={app.busy}
                onclick={() => app.readFirewall()}>{app.t("Retry")}</Button
              >{/if}
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
              ><Regions {app} {compact} onmanage={manage} /></Tabs.Content
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
      {#if compact || bar.visible || app.firewallAction || app.firewallFeedback}<footer
          class={compact
            ? "flex shrink-0 flex-col gap-2 border-t bg-background px-4 py-2.5"
            : "flex shrink-0 flex-wrap items-center justify-between gap-4 border-t bg-background px-7 py-3"}
        >
          {#if bar.visible || app.firewallAction || app.firewallFeedback}<div
              class="min-w-0 flex-1"
            >
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
              {#if bar.apply || app.firewallAction === "apply"}<span
                  class="text-[11px] text-muted-foreground"
                  title={app.t(scopeWarning(app.status))}
                  >{app.t(
                    executableBlocking(app.status)
                      ? "Selected games"
                      : "All apps on this PC",
                  )}</span
                >{/if}
              {#if bar.apply && missing.length}<div
                  class="mt-1 flex flex-wrap items-center gap-x-2 gap-y-1 text-xs"
                >
                  <p class="text-destructive">
                    {app.t("Game executable needed for")}:
                    <span dir="auto"
                      >{missing
                        .map((target) => target.gameName)
                        .join(", ")}</span
                    >
                  </p>
                  <Button
                    variant="link"
                    class="h-auto p-0 text-xs"
                    disabled={app.busy || changingLayout}
                    onclick={showSettings}>{app.t("Open Settings")}</Button
                  >
                </div>{/if}
            </div>{/if}
          {#if compact}
            <div class="flex items-center justify-between gap-2">
              <Button
                variant="outline"
                size="sm"
                disabled={app.busy || !app.status}
                onclick={() => (confirmOpen = true)}
                ><RotateCcw />{app.t("Reset")}</Button
              >
              <div class="flex min-w-0 items-center gap-2">
                <button
                  type="button"
                  class="truncate text-[10px] text-muted-foreground transition-colors hover:text-foreground focus-visible:text-foreground"
                  dir="ltr"
                  onclick={() => (aboutOpen = true)}
                  >LobbyLocker v{version}</button
                >
                {@render themeToggle()}
              </div>
            </div>
          {/if}
        </footer>{/if}
    </main>
    {#if !compact}
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
    {/if}
  </div>
</Tabs.Root>
<Dialog.Root bind:open={updatePromptOpen}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>{app.t("Update available")}</Dialog.Title>
      <Dialog.Description>
        {app.t("LobbyLocker {current} → {next}", {
          current: version,
          next: app.updateVersion,
        })}
      </Dialog.Description>
    </Dialog.Header>
    {#if app.updateNotes}<p
        class="text-xs text-muted-foreground whitespace-pre-line"
        dir="auto"
      >
        {app.updateNotes}
      </p>{/if}
    <Dialog.Footer class="gap-2 sm:gap-0">
      <Button variant="outline" onclick={() => (updatePromptOpen = false)}>
        {app.t("Later")}
      </Button>
      <Button
        disabled={app.busy || app.updateInstalling}
        onclick={() => {
          void app.installUpdate();
          updatePromptOpen = false;
        }}
      >
        {app.t("Update & restart")}
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
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
<AboutDialog {app} {version} {dark} bind:open={aboutOpen} />
