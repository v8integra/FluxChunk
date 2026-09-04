<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { open } from "@tauri-apps/plugin-dialog";
  import CollectionTree, { type TreeNode } from "$lib/CollectionTree.svelte";
  import ConsolePanel from "$lib/ConsolePanel.svelte";
  import CrashDialog, { type CrashInfo } from "$lib/CrashDialog.svelte";
  import ImportDialog, { type ImportPreviewDto } from "$lib/ImportDialog.svelte";
  import RequestErrorPanel from "$lib/RequestErrorPanel.svelte";
  import ResponsePanel from "$lib/ResponsePanel.svelte";
  import SettingsPanel from "$lib/SettingsPanel.svelte";
  import ThemeSwitcher, { type Theme } from "$lib/ThemeSwitcher.svelte";
  import Tour, { type TourStep } from "$lib/Tour.svelte";
  import UpdateDialog, { type UpdateInfo, type UpdateStage } from "$lib/UpdateDialog.svelte";
  import type { ConsoleEntryDto, RequestFailureDto, SendResponseResult } from "$lib/types";

  type EnvironmentSummary = {
    name: string;
    vars: Record<string, string>;
  };

  type AuthMode = "none" | "inherit" | "basic" | "bearer" | "apikey" | "oauth2";

  type AuthPayloadDto = {
    mode: string;
    username?: string | null;
    password?: string | null;
    token?: string | null;
    key?: string | null;
    value?: string | null;
    placement?: string | null;
    accessToken?: string | null;
  };

  type RequestSummary = {
    name: string;
    method: string;
    url: string;
    headers: Record<string, string>;
    auth: AuthPayloadDto;
    body: string | null;
    script_pre_request: string | null;
    script_post_response: string | null;
  };

  type CollectionSummary = {
    name: string;
    root: string;
    items: TreeNode[];
    environments: { name: string; path: string }[];
  };

  type ImportSummaryDto = {
    name: string;
    collection_path: string;
    request_count: number;
    warnings: string[];
  };

  type Tab = {
    id: string;
    title: string;
    filePath: string | null;
    method: string;
    url: string;
    headersText: string;
    authMode: AuthMode;
    authUsername: string;
    authPassword: string;
    authToken: string;
    authApiKeyName: string;
    authApiKeyValue: string;
    authApiKeyPlacement: string;
    authAccessToken: string;
    body: string;
    scriptPreRequest: string;
    scriptPostResponse: string;
    console: ConsoleEntryDto[];
    sending: boolean;
    saving: boolean;
    error: string;
    errorKind: string;
    response: SendResponseResult | null;
    savedSnapshot: string;
  };

  function makeId(): string {
    return Math.random().toString(36).slice(2);
  }

  function newAdHocTab(): Tab {
    return {
      id: makeId(),
      title: "Untitled request",
      filePath: null,
      method: "GET",
      url: "https://api.wheretheiss.at/v1/satellites/25544",
      headersText: "Accept: application/json",
      authMode: "none",
      authUsername: "",
      authPassword: "",
      authToken: "",
      authApiKeyName: "",
      authApiKeyValue: "",
      authApiKeyPlacement: "header",
      authAccessToken: "",
      body: "",
      scriptPreRequest: "",
      scriptPostResponse: "",
      console: [],
      sending: false,
      saving: false,
      error: "",
      errorKind: "",
      response: null,
      savedSnapshot: "",
    };
  }

  let tabs = $state<Tab[]>([newAdHocTab()]);
  let activeTabId = $state(tabs[0].id);
  let activeTab = $derived(tabs.find((t) => t.id === activeTabId) ?? null);

  let environment = $state<EnvironmentSummary | null>(null);
  let environmentError = $state("");

  let collectionName = $state<string | null>(null);
  let collectionItems = $state<TreeNode[]>([]);
  let collectionEnvironments = $state<{ name: string; path: string }[]>([]);
  let sidebarError = $state("");

  // --- workspace shell: theme, layout, panel visibility (spec section 10) ---
  // Persisted as tier-1 settings (spec section 11) -- see
  // apps/desktop/src-tauri/src/settings.rs. The 3 fixed presets (Split/
  // Stacked/Focus) are fully implemented; the spec's 4th "Custom" slot
  // (drag-and-drop panel rearrangement, with its own Save/Cancel
  // interaction) is a deliberate follow-up, not attempted here -- real
  // drag-and-drop layout editing is its own substantial feature, and a
  // button that just duplicated Split without it would be a hollow stand-in.

  type LayoutPreset = "split" | "stacked" | "focus";
  const LAYOUT_PRESETS: LayoutPreset[] = ["split", "stacked", "focus"];

  type PanelVisibility = { headers: boolean; auth: boolean; body: boolean; scripts: boolean; console: boolean };
  type SettingsDto = {
    theme: Theme;
    layout_preset: LayoutPreset;
    panels: PanelVisibility;
    has_seen_tour: boolean;
    auto_check_updates: boolean;
    update_check_url: string;
    verbose_logging: boolean;
  };

  let theme = $state<Theme>("light");
  let layoutPreset = $state<LayoutPreset>("stacked");
  let panels = $state<PanelVisibility>({ headers: true, auth: true, body: true, scripts: true, console: true });
  let scriptSubTab = $state<"pre-request" | "post-response">("pre-request");
  let hasSeenTour = $state(false);
  let autoCheckUpdates = $state(false);
  let updateCheckUrl = $state("");
  let verboseLogging = $state(false);
  let settingsLoaded = $state(false);

  $effect(() => {
    document.documentElement.dataset.theme = theme;
  });

  $effect(() => {
    // Tracked dependencies: re-save whenever any of these change.
    void theme;
    void layoutPreset;
    void panels.headers;
    void panels.auth;
    void panels.body;
    void hasSeenTour;
    void autoCheckUpdates;
    void updateCheckUrl;
    void verboseLogging;
    if (!settingsLoaded) return; // don't clobber the saved file with defaults before the initial load lands
    invoke("save_settings", {
      settings: {
        theme,
        layout_preset: layoutPreset,
        panels,
        has_seen_tour: hasSeenTour,
        auto_check_updates: autoCheckUpdates,
        update_check_url: updateCheckUrl,
        verbose_logging: verboseLogging,
      },
    }).catch((e) => console.error(e));
  });

  async function loadSettings() {
    try {
      const s = await invoke<SettingsDto>("load_settings");
      theme = s.theme;
      layoutPreset = s.layout_preset;
      panels = s.panels;
      hasSeenTour = s.has_seen_tour;
      autoCheckUpdates = s.auto_check_updates;
      updateCheckUrl = s.update_check_url;
      verboseLogging = s.verbose_logging;
      // Spec section 12: "Auto-launches on first install" -- and only
      // then; the persistent "Take the tour" control below covers anyone
      // who wants it again later.
      if (!hasSeenTour) {
        tourStep = 0;
        showTour = true;
      }
      // Spec section 13: "automatic checks OFF by default" -- only check
      // here at all if the user turned this on. Silent either way: no
      // dialog for "you're up to date", and a failed background check
      // just logs rather than surfacing an alarming popup on launch.
      if (autoCheckUpdates) {
        checkForUpdates(true);
      }
    } catch (e) {
      console.error(e);
    } finally {
      settingsLoaded = true;
    }
  }
  loadSettings();

  // --- updates (spec section 13) ---

  let updateStage = $state<UpdateStage | null>(null);
  let updateInfo = $state<UpdateInfo | null>(null);
  let updateError = $state("");
  let showSettingsPanel = $state(false);

  async function checkForUpdates(silent = false) {
    if (!silent) {
      updateStage = "checking";
      updateError = "";
    }
    try {
      const info = await invoke<UpdateInfo | null>("check_for_updates");
      if (info) {
        updateInfo = info;
        updateStage = "available";
      } else if (!silent) {
        updateStage = "up-to-date";
      }
    } catch (e) {
      if (silent) {
        console.error(e);
      } else {
        updateError = String(e);
        updateStage = "error";
      }
    }
  }

  async function approveDownload() {
    updateStage = "downloading";
    updateError = "";
    try {
      await invoke("download_update");
      updateStage = "ready";
    } catch (e) {
      updateError = String(e);
      updateStage = "error";
    }
  }

  async function installAndRestart() {
    try {
      await invoke("install_and_restart");
      // The app exits and relaunches from here -- nothing left to do.
    } catch (e) {
      updateError = String(e);
    }
  }

  function dismissUpdateDialog() {
    updateStage = null;
    updateInfo = null;
    updateError = "";
  }

  function onSettingsChange(auto: boolean, url: string) {
    autoCheckUpdates = auto;
    updateCheckUrl = url;
  }

  function onVerboseLoggingChange(verbose: boolean) {
    verboseLogging = verbose;
  }

  // --- crash reporting (spec section 16) ---

  let pendingCrash = $state<CrashInfo | null>(null);

  async function checkPendingCrash() {
    try {
      pendingCrash = await invoke<CrashInfo | null>("check_pending_crash");
    } catch (e) {
      console.error(e);
    }
  }
  checkPendingCrash();

  function loadCrashDetails(): Promise<string> {
    if (!pendingCrash) return Promise.reject("no crash report");
    return invoke<string>("read_crash_report", { path: pendingCrash.path });
  }

  function dismissCrashDialog() {
    pendingCrash = null;
  }

  // --- local identity (spec section 6) ---

  type LocalIdentityInfo = { public_key: string; display_name: string };

  let identityPublicKey = $state("");
  let identityDisplayName = $state("");

  async function loadIdentity() {
    try {
      const info = await invoke<LocalIdentityInfo>("get_identity");
      identityPublicKey = info.public_key;
      identityDisplayName = info.display_name;
    } catch (e) {
      console.error(e);
    }
  }
  loadIdentity();

  async function changeDisplayName(name: string) {
    try {
      const info = await invoke<LocalIdentityInfo>("set_display_name", { displayName: name });
      identityPublicKey = info.public_key;
      identityDisplayName = info.display_name;
    } catch (e) {
      console.error(e);
    }
  }

  // --- first-run tour (spec section 12) ---

  const TOUR_STEPS: TourStep[] = [
    {
      title: "Welcome to FluxChunk",
      body: "A local-first, Git-native API client. Five short steps to get oriented -- skip anytime.",
    },
    {
      title: "Send a real request",
      body: "You're already set up with a live request to the International Space Station's tracking API. Hit Send below and watch it actually happen.",
    },
    {
      title: "Read the response",
      body: "That's real, live data -- not a mock. Pretty gives you a structured, searchable tree; Raw shows the exact bytes; History keeps every past run.",
    },
    {
      title: "Where collections live",
      body: "The sidebar on the left is where you open, import, or build multi-request collections -- plain text files you can put straight into Git.",
    },
    {
      title: "Make it yours",
      body: "Theme and layout live in the top right. Pick a look and a panel arrangement that works for you.",
    },
  ];

  let showTour = $state(false);
  let tourStep = $state(0);

  function startTour() {
    tourStep = 0;
    showTour = true;
  }

  function endTour() {
    showTour = false;
    hasSeenTour = true;
  }

  function capitalize(s: string): string {
    return s[0].toUpperCase() + s.slice(1);
  }

  function parseHeaders(text: string): Record<string, string> {
    const headers: Record<string, string> = {};
    for (const line of text.split("\n")) {
      const trimmed = line.trim();
      if (!trimmed) continue;
      const idx = trimmed.indexOf(":");
      if (idx === -1) continue;
      headers[trimmed.slice(0, idx).trim()] = trimmed.slice(idx + 1).trim();
    }
    return headers;
  }

  function authPayload(tab: Tab) {
    return {
      mode: tab.authMode,
      username: tab.authUsername || null,
      password: tab.authPassword || null,
      token: tab.authToken || null,
      key: tab.authApiKeyName || null,
      value: tab.authApiKeyValue || null,
      placement: tab.authApiKeyPlacement,
      accessToken: tab.authAccessToken || null,
    };
  }

  function savableFields(tab: Tab) {
    return {
      method: tab.method,
      url: tab.url,
      headersText: tab.headersText,
      authMode: tab.authMode,
      authUsername: tab.authUsername,
      authPassword: tab.authPassword,
      authToken: tab.authToken,
      authApiKeyName: tab.authApiKeyName,
      authApiKeyValue: tab.authApiKeyValue,
      authApiKeyPlacement: tab.authApiKeyPlacement,
      authAccessToken: tab.authAccessToken,
      body: tab.body,
      scriptPreRequest: tab.scriptPreRequest,
      scriptPostResponse: tab.scriptPostResponse,
    };
  }

  function snapshotOf(tab: Tab): string {
    return JSON.stringify(savableFields(tab));
  }

  function isDirty(tab: Tab): boolean {
    return tab.filePath !== null && snapshotOf(tab) !== tab.savedSnapshot;
  }

  /** Groups response history per request: a saved request's file path is
   * stable across sends and even app restarts; an ad-hoc tab has no file,
   * so its own (session-local) tab id keeps its history separate from
   * every other ad-hoc tab instead of merging them all together. */
  function requestKeyOf(tab: Tab): string {
    return tab.filePath ?? tab.id;
  }

  // --- environments ---

  async function loadEnvironment() {
    environmentError = "";
    let path: string | null;
    try {
      path = await open({
        multiple: false,
        directory: false,
        filters: [{ name: "FluxChunk environment", extensions: ["apienv"] }],
      });
    } catch (e) {
      environmentError = String(e);
      return;
    }
    if (!path) return;
    await loadEnvironmentByPath(path);
  }

  async function loadEnvironmentByPath(path: string) {
    environmentError = "";
    try {
      environment = await invoke<EnvironmentSummary>("load_environment", { path });
    } catch (e) {
      environmentError = String(e);
    }
  }

  async function clearEnvironment() {
    await invoke("clear_environment");
    environment = null;
    environmentError = "";
  }

  // --- collections ---

  async function openCollection() {
    sidebarError = "";
    let path: string | null;
    try {
      path = await open({ directory: true, multiple: false });
    } catch (e) {
      sidebarError = String(e);
      return;
    }
    if (!path) return;

    try {
      const summary = await invoke<CollectionSummary>("open_collection", { path });
      collectionName = summary.name;
      collectionItems = summary.items;
      collectionEnvironments = summary.environments;
    } catch (e) {
      sidebarError = String(e);
    }
  }

  async function closeCollection() {
    await invoke("close_collection");
    collectionName = null;
    collectionItems = [];
    collectionEnvironments = [];
  }

  // --- import (Postman / OpenAPI) ---
  // Spec section 8's two-dialog flow: preview (parse + security scan, no
  // writes) -> ImportDialog shows a summary, then findings if any were
  // scanned up -> commit (re-parses and writes, honoring the user's
  // choice). "Reject Import" and plain Cancel never call commit at all,
  // so nothing is ever written for either.

  let importSummary = $state<ImportSummaryDto | null>(null);
  let importKind = $state<"postman" | "openapi">("postman");
  let importSourcePath = $state("");
  let importParentDir = $state("");
  let importStage = $state<"summary" | "findings" | null>(null);
  let importPreview = $state<ImportPreviewDto | null>(null);
  let importCommitting = $state(false);
  let importDialogError = $state("");

  async function importCollection(kind: "postman" | "openapi") {
    sidebarError = "";
    importSummary = null;

    let sourcePath: string | null;
    try {
      sourcePath = await open({
        multiple: false,
        directory: false,
        filters: [{ name: kind === "postman" ? "Postman Collection" : "OpenAPI / Swagger", extensions: ["json"] }],
      });
    } catch (e) {
      sidebarError = String(e);
      return;
    }
    if (!sourcePath) return;

    // Import creates a new folder (named after the collection) inside
    // whatever directory is picked here, rather than writing directly
    // into it -- avoids dumping collection.apicol and request folders
    // into a directory that might already hold unrelated files.
    let parentDir: string | null;
    try {
      parentDir = await open({ directory: true, multiple: false });
    } catch (e) {
      sidebarError = String(e);
      return;
    }
    if (!parentDir) return;

    try {
      const previewCommand = kind === "postman" ? "preview_postman_import" : "preview_openapi_import";
      const preview = await invoke<ImportPreviewDto>(previewCommand, { sourcePath });
      importKind = kind;
      importSourcePath = sourcePath;
      importParentDir = parentDir;
      importPreview = preview;
      importDialogError = "";
      importStage = "summary";
    } catch (e) {
      sidebarError = String(e);
    }
  }

  function cancelImportDialog() {
    importStage = null;
    importPreview = null;
  }

  function scanContinue() {
    if (!importPreview) return;
    if (importPreview.security_findings.length > 0) {
      importStage = "findings";
    } else {
      commitImport(false);
    }
  }

  async function commitImport(skipFlagged: boolean) {
    if (!importPreview) return;
    importCommitting = true;
    importDialogError = "";
    try {
      const commitCommand = importKind === "postman" ? "commit_postman_import" : "commit_openapi_import";
      const summary = await invoke<ImportSummaryDto>(commitCommand, {
        sourcePath: importSourcePath,
        parentDir: importParentDir,
        skipFlagged,
      });
      importSummary = summary;
      importStage = null;
      importPreview = null;

      const opened = await invoke<CollectionSummary>("open_collection", { path: summary.collection_path });
      collectionName = opened.name;
      collectionItems = opened.items;
      collectionEnvironments = opened.environments;
    } catch (e) {
      importDialogError = String(e);
    } finally {
      importCommitting = false;
    }
  }

  // --- tabs ---

  function addTab() {
    const tab = newAdHocTab();
    tabs.push(tab);
    activeTabId = tab.id;
  }

  function closeTab(id: string) {
    const idx = tabs.findIndex((t) => t.id === id);
    if (idx === -1) return;
    const tab = tabs[idx];
    if (isDirty(tab) && !confirm(`Discard unsaved changes to "${tab.title}"?`)) return;

    tabs.splice(idx, 1);
    if (tabs.length === 0) {
      const fresh = newAdHocTab();
      tabs.push(fresh);
      activeTabId = fresh.id;
    } else if (activeTabId === id) {
      activeTabId = tabs[Math.max(0, idx - 1)].id;
    }
  }

  async function openRequestInTab(path: string) {
    sidebarError = "";
    const existing = tabs.find((t) => t.filePath === path);
    if (existing) {
      activeTabId = existing.id;
      return;
    }
    try {
      const summary = await invoke<RequestSummary>("read_request", { path });
      const tab: Tab = {
        id: makeId(),
        title: summary.name || path.split(/[\\/]/).pop() || "Request",
        filePath: path,
        method: summary.method,
        url: summary.url,
        headersText: Object.entries(summary.headers)
          .map(([k, v]) => `${k}: ${v}`)
          .join("\n"),
        authMode: summary.auth.mode as AuthMode,
        authUsername: summary.auth.username ?? "",
        authPassword: summary.auth.password ?? "",
        authToken: summary.auth.token ?? "",
        authApiKeyName: summary.auth.key ?? "",
        authApiKeyValue: summary.auth.value ?? "",
        authApiKeyPlacement: summary.auth.placement ?? "header",
        authAccessToken: summary.auth.accessToken ?? "",
        body: summary.body ?? "",
        scriptPreRequest: summary.script_pre_request ?? "",
        scriptPostResponse: summary.script_post_response ?? "",
        console: [],
        sending: false,
        saving: false,
        error: "",
        errorKind: "",
        response: null,
        savedSnapshot: "",
      };
      tab.savedSnapshot = snapshotOf(tab);
      tabs.push(tab);
      activeTabId = tab.id;
    } catch (e) {
      sidebarError = String(e);
    }
  }

  async function saveTab(tab: Tab) {
    if (!tab.filePath) return;
    tab.saving = true;
    tab.errorKind = "";
    try {
      await invoke("save_request", {
        path: tab.filePath,
        method: tab.method,
        url: tab.url,
        headers: parseHeaders(tab.headersText),
        auth: authPayload(tab),
        body: tab.body || null,
        scriptPreRequest: tab.scriptPreRequest || null,
        scriptPostResponse: tab.scriptPostResponse || null,
      });
      tab.savedSnapshot = snapshotOf(tab);
      tab.error = "";
    } catch (e) {
      tab.error = String(e);
    } finally {
      tab.saving = false;
    }
  }

  async function sendTab(event: Event, tab: Tab) {
    event.preventDefault();
    tab.sending = true;
    tab.error = "";
    tab.errorKind = "";
    tab.response = null;
    tab.console = [];
    try {
      tab.response = await invoke<SendResponseResult>("send_request", {
        requestKey: requestKeyOf(tab),
        requestLabel: tab.title,
        method: tab.method,
        url: tab.url,
        headers: parseHeaders(tab.headersText),
        body: tab.body || null,
        auth: authPayload(tab),
        scriptPreRequest: tab.scriptPreRequest || null,
        scriptPostResponse: tab.scriptPostResponse || null,
      });
      tab.console = tab.response.console;
    } catch (e) {
      // send_request rejects with a { kind, message, console } object
      // (spec section 16's categorized failures) -- fall back to a
      // plain string for anything else Tauri might reject with.
      const failure = e as RequestFailureDto | string;
      if (typeof failure === "object" && failure && "kind" in failure) {
        tab.errorKind = failure.kind;
        tab.error = failure.message;
        tab.console = failure.console ?? [];
      } else {
        tab.errorKind = "other";
        tab.error = String(e);
      }
    } finally {
      tab.sending = false;
    }
  }

  function handleKeydown(event: KeyboardEvent) {
    if ((event.ctrlKey || event.metaKey) && event.key === "s") {
      event.preventDefault();
      if (activeTab?.filePath) saveTab(activeTab);
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="app">
  {#if layoutPreset !== "focus"}
    <aside class="sidebar">
      <div class="sidebar-toolbar">
        <button type="button" onclick={openCollection}>Open Collection...</button>
        <button type="button" onclick={() => importCollection("postman")}>Import Postman...</button>
        <button type="button" onclick={() => importCollection("openapi")}>Import OpenAPI...</button>
      </div>

      {#if collectionName}
        <div class="sidebar-header">
          <strong>{collectionName}</strong>
          <button type="button" onclick={closeCollection}>Close</button>
        </div>
        {#if collectionEnvironments.length > 0}
          <div class="env-quick-pick">
            {#each collectionEnvironments as env (env.path)}
              <button type="button" onclick={() => loadEnvironmentByPath(env.path)}>{env.name}</button>
            {/each}
          </div>
        {/if}
        <CollectionTree items={collectionItems} onSelect={openRequestInTab} activePath={activeTab?.filePath ?? null} />
      {:else}
        <p class="hint">No collection open.</p>
      {/if}

      {#if importSummary}
        <div class="import-summary">
          <p>Imported {importSummary.request_count} request(s) into "{importSummary.name}".</p>
          {#if importSummary.warnings.length > 0}
            <details>
              <summary>{importSummary.warnings.length} warning(s)</summary>
              <ul>
                {#each importSummary.warnings as w, i (i)}
                  <li>{w}</li>
                {/each}
              </ul>
            </details>
          {/if}
        </div>
      {/if}

      {#if sidebarError}
        <p class="error">{sidebarError}</p>
      {/if}
    </aside>
  {/if}

  {#if importStage && importPreview}
    <ImportDialog
      stage={importStage}
      preview={importPreview}
      committing={importCommitting}
      error={importDialogError}
      onCancel={cancelImportDialog}
      onScanContinue={scanContinue}
      onRejectImport={cancelImportDialog}
      onImportSkipFlagged={() => commitImport(true)}
      onImportAnyway={() => commitImport(false)}
    />
  {/if}

  <main>
    <div class="workspace-header">
      <h1>FluxChunk</h1>
      <div class="workspace-controls">
        <div class="layout-presets" role="radiogroup" aria-label="Layout">
          {#each LAYOUT_PRESETS as p (p)}
            <button type="button" class:active={layoutPreset === p} onclick={() => (layoutPreset = p)}>{capitalize(p)}</button>
          {/each}
        </div>
        <ThemeSwitcher {theme} onChange={(t) => (theme = t)} />
        <button type="button" class="tour-button" title="Take the tour" onclick={startTour}>?</button>
        <button type="button" class="update-button" title="Check for Updates" onclick={() => checkForUpdates(false)}>&#8635;</button>
        <button type="button" class="settings-button" title="Settings" onclick={() => (showSettingsPanel = true)}>&#9881;</button>
      </div>
    </div>

    <section class="environment">
      {#if environment}
        <span>Environment: <strong>{environment.name}</strong></span>
        <button type="button" onclick={clearEnvironment}>Clear</button>
        {#if Object.keys(environment.vars).length > 0}
          <details>
            <summary>{Object.keys(environment.vars).length} variable(s)</summary>
            <ul>
              {#each Object.entries(environment.vars) as [key, value] (key)}
                <li><code>{key}</code> = <code>{value}</code></li>
              {/each}
            </ul>
          </details>
        {/if}
      {:else}
        <span>No environment loaded</span>
        <button type="button" onclick={loadEnvironment}>Load .apienv...</button>
      {/if}
    </section>
    {#if environmentError}
      <p class="error">{environmentError}</p>
    {/if}

    <div class="tab-strip">
      {#each tabs as tab (tab.id)}
        <div class="tab" class:active={tab.id === activeTabId}>
          <button type="button" class="tab-select" onclick={() => (activeTabId = tab.id)}>
            <span class="tab-title">{tab.title}</span>
            {#if isDirty(tab)}<span class="dirty-dot" title="Unsaved changes">&bull;</span>{/if}
          </button>
          <button type="button" class="tab-close" title="Close tab" onclick={() => closeTab(tab.id)}>&times;</button>
        </div>
      {/each}
      <button type="button" class="new-tab" title="New request" onclick={addTab}>+</button>
    </div>

    {#if activeTab}
      {@const tab = activeTab}
      <form onsubmit={(e) => sendTab(e, tab)} class="request-row">
        <select bind:value={tab.method}>
          <option>GET</option>
          <option>POST</option>
          <option>PUT</option>
          <option>PATCH</option>
          <option>DELETE</option>
          <option>HEAD</option>
          <option>OPTIONS</option>
        </select>
        <input class="url" bind:value={tab.url} placeholder="https://api.example.com/..." />
        {#if tab.filePath}
          <button type="button" onclick={() => saveTab(tab)} disabled={tab.saving || !isDirty(tab)}>
            {tab.saving ? "Saving..." : "Save"}
          </button>
        {/if}
        <button type="submit" disabled={tab.sending}>{tab.sending ? "Sending..." : "Send"}</button>
      </form>

      <!-- Panel show/hide chips (spec section 10): hiding never deletes
           data, it's purely a visibility toggle -- the tab's fields stay
           exactly as they are underneath. -->
      <div class="panel-chips">
        <button type="button" class="chip" class:active={panels.headers} onclick={() => (panels.headers = !panels.headers)}>Headers</button>
        <button type="button" class="chip" class:active={panels.auth} onclick={() => (panels.auth = !panels.auth)}>Auth</button>
        <button type="button" class="chip" class:active={panels.body} onclick={() => (panels.body = !panels.body)}>Body</button>
        <button type="button" class="chip" class:active={panels.scripts} onclick={() => (panels.scripts = !panels.scripts)}>Scripts</button>
        <button type="button" class="chip" class:active={panels.console} onclick={() => (panels.console = !panels.console)}>Console</button>
      </div>

      <div class="content" class:split={layoutPreset === "split"}>
        <div class="request-builder">
          {#if panels.headers}
            <section>
              <label for="headers">Headers (one per line, "Key: Value")</label>
              <textarea id="headers" bind:value={tab.headersText} rows="3"></textarea>
            </section>
          {/if}

          {#if panels.auth}
            <section class="auth">
              <label for="auth-mode">Auth</label>
              <select id="auth-mode" bind:value={tab.authMode}>
                <option value="none">None</option>
                <option value="inherit">Inherit</option>
                <option value="basic">Basic</option>
                <option value="bearer">Bearer</option>
                <option value="apikey">API Key</option>
                <option value="oauth2">OAuth2</option>
              </select>

              {#if tab.authMode === "inherit"}
                <p class="hint">
                  {collectionName ? `Inherits "${collectionName}"'s auth.` : "No collection open -- resolves the same as None."}
                </p>
              {:else if tab.authMode === "basic"}
                <div class="auth-fields">
                  <input bind:value={tab.authUsername} placeholder="Username" />
                  <input bind:value={tab.authPassword} type="password" placeholder="Password" />
                </div>
              {:else if tab.authMode === "bearer"}
                <div class="auth-fields">
                  <input bind:value={tab.authToken} type="password" placeholder="Token" />
                </div>
              {:else if tab.authMode === "apikey"}
                <div class="auth-fields">
                  <input bind:value={tab.authApiKeyName} placeholder="Key name (e.g. X-API-Key)" />
                  <input bind:value={tab.authApiKeyValue} type="password" placeholder="Value" />
                  <select bind:value={tab.authApiKeyPlacement}>
                    <option value="header">Header</option>
                    <option value="query">Query param</option>
                  </select>
                </div>
              {:else if tab.authMode === "oauth2"}
                <div class="auth-fields">
                  <input bind:value={tab.authAccessToken} type="password" placeholder="Access token" />
                </div>
                <p class="hint">Paste a token you already have -- interactive login isn't implemented yet.</p>
              {/if}
              <p class="hint">Auth values may use {"{{var}}"} / {"{{vault:...}}"} too, same as headers.</p>
            </section>
          {/if}

          {#if panels.body}
            <section>
              <label for="body">Body</label>
              <textarea id="body" bind:value={tab.body} rows="5" placeholder="(optional)"></textarea>
            </section>
          {/if}

          {#if panels.scripts}
            <section>
              <div class="sub-tabs" role="tablist" aria-label="Scripts">
                <button type="button" class:active={scriptSubTab === "pre-request"} onclick={() => (scriptSubTab = "pre-request")}>
                  Pre-request
                </button>
                <button type="button" class:active={scriptSubTab === "post-response"} onclick={() => (scriptSubTab = "post-response")}>
                  Post-response
                </button>
              </div>
              {#if scriptSubTab === "pre-request"}
                <label for="script-pre-request">Runs before the request is sent -- can read/set vars and modify the request (req.url, req.headers, req.body).</label>
                <textarea id="script-pre-request" class="script" bind:value={tab.scriptPreRequest} rows="6" placeholder={'bru.setVar("request_time", Date.now());'}
                ></textarea>
              {:else}
                <label for="script-post-response">Runs after the response comes back -- can read the response (res.status, res.headers, res.body) and set vars.</label>
                <textarea id="script-post-response" class="script" bind:value={tab.scriptPostResponse} rows="6" placeholder={'if (res.status === 200) {\n  bru.setVar("last_id", res.body.id);\n}'}
                ></textarea>
              {/if}
              <p class="hint">Vault secrets are never readable here -- a {"{{vault:...}}"} reference stays literal text until the engine resolves it at send time.</p>
            </section>
          {/if}

          {#if panels.console}
            <section>
              <label for="console-panel">Console</label>
              <div id="console-panel">
                <ConsolePanel entries={tab.console} />
              </div>
            </section>
          {/if}

          {#if tab.error && !tab.errorKind}
            <p class="error">{tab.error}</p>
          {/if}
        </div>

        {#if tab.response}
          <div class="response-column">
            <ResponsePanel response={tab.response} requestKey={requestKeyOf(tab)} />
          </div>
        {:else if tab.error && tab.errorKind === "script"}
          <div class="response-column">
            <p class="script-error-hint">Pre-request script failed -- see the Console panel.</p>
          </div>
        {:else if tab.error && tab.errorKind}
          <div class="response-column">
            <RequestErrorPanel kind={tab.errorKind} message={tab.error} />
          </div>
        {/if}
      </div>
    {/if}
  </main>
</div>

{#if showTour}
  <Tour steps={TOUR_STEPS} step={tourStep} onNext={() => tourStep++} onBack={() => tourStep--} onSkip={endTour} onFinish={endTour} />
{/if}

{#if updateStage}
  <UpdateDialog
    stage={updateStage}
    info={updateInfo}
    error={updateError}
    onApproveDownload={approveDownload}
    onInstallAndRestart={installAndRestart}
    onDismiss={dismissUpdateDialog}
  />
{/if}

{#if showSettingsPanel}
  <SettingsPanel
    {autoCheckUpdates}
    {updateCheckUrl}
    {verboseLogging}
    publicKey={identityPublicKey}
    displayName={identityDisplayName}
    onChange={onSettingsChange}
    {onVerboseLoggingChange}
    onDisplayNameChange={changeDisplayName}
    onClose={() => (showSettingsPanel = false)}
    onCheckNow={() => checkForUpdates(false)}
  />
{/if}

{#if pendingCrash}
  <CrashDialog crash={pendingCrash} onLoadDetails={loadCrashDetails} onDismiss={dismissCrashDialog} />
{/if}

<style>
  :global(body) {
    font-family: Inter, Avenir, Helvetica, Arial, sans-serif;
  }

  .app {
    display: flex;
    align-items: flex-start;
    min-height: 100vh;
  }

  .sidebar {
    width: 220px;
    flex-shrink: 0;
    padding: 1.5rem 0.75rem;
    box-sizing: border-box;
    border-right: 1px solid var(--border);
    min-height: 100vh;
  }

  .sidebar-toolbar {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    margin-bottom: 0.75rem;
  }

  .sidebar-toolbar button {
    font-size: 0.8rem;
    padding: 0.3rem 0.5rem;
    text-align: left;
  }

  .sidebar-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    font-size: 0.9rem;
    margin-bottom: 0.5rem;
  }

  .import-summary {
    margin-top: 0.75rem;
    padding-top: 0.5rem;
    border-top: 1px solid var(--border);
    font-size: 0.8rem;
  }

  .import-summary ul {
    margin: 0.25rem 0 0;
    padding-left: 1.1rem;
  }

  .env-quick-pick {
    display: flex;
    flex-wrap: wrap;
    gap: 0.3rem;
    margin-bottom: 0.75rem;
  }

  .env-quick-pick button {
    font-size: 0.78rem;
    padding: 0.15rem 0.45rem;
  }

  main {
    flex: 1;
    max-width: 1200px;
    margin: 0 auto;
    padding: 2rem 1.5rem;
  }

  .workspace-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 1rem;
  }

  .workspace-header h1 {
    margin: 0;
  }

  .workspace-controls {
    display: flex;
    align-items: flex-start;
    gap: 0.6rem;
  }

  .tour-button,
  .update-button,
  .settings-button {
    width: 1.9rem;
    height: 1.9rem;
    padding: 0;
    border-radius: 50%;
    font-weight: 700;
  }

  .layout-presets {
    display: flex;
    gap: 0.2rem;
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 0.2rem;
    height: fit-content;
  }

  .layout-presets button {
    font-size: 0.78rem;
    padding: 0.2rem 0.5rem;
    border: none;
    background: transparent;
  }

  .layout-presets button.active {
    background: var(--accent);
    color: var(--accent-text);
  }

  .tab-strip {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    flex-wrap: wrap;
    margin-bottom: 0.75rem;
    border-bottom: 1px solid var(--border);
    padding-bottom: 0.4rem;
  }

  .tab {
    display: flex;
    align-items: center;
    gap: 0.15rem;
    border-radius: 6px 6px 0 0;
    background: transparent;
    border: 1px solid transparent;
    padding: 0.15rem 0.15rem 0.15rem 0.5rem;
  }

  .tab.active {
    background: var(--bg-hover);
    border-color: var(--border);
    border-bottom-color: transparent;
  }

  .tab-select {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    font-size: 0.85rem;
    padding: 0.15rem 0.2rem;
    background: transparent;
    border: none;
  }

  .tab-title {
    max-width: 12rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .dirty-dot {
    color: var(--warning);
    font-size: 1.1rem;
    line-height: 0;
  }

  .tab-close {
    opacity: 0.6;
    padding: 0.15rem 0.4rem;
    background: transparent;
    border: none;
  }

  .tab-close:hover {
    opacity: 1;
  }

  .new-tab {
    padding: 0.3rem 0.6rem;
  }

  .request-row {
    display: flex;
    gap: 0.5rem;
  }

  .request-row .url {
    flex: 1;
  }

  .panel-chips {
    display: flex;
    gap: 0.35rem;
    margin-top: 0.75rem;
  }

  .chip {
    font-size: 0.78rem;
    padding: 0.2rem 0.6rem;
    border-radius: 999px;
    background: transparent;
    opacity: 0.55;
  }

  .chip.active {
    opacity: 1;
    background: var(--bg-hover);
    border-color: var(--accent);
  }

  .content {
    display: block;
  }

  .content.split {
    display: flex;
    gap: 1.5rem;
    align-items: flex-start;
  }

  .content.split .request-builder,
  .content.split .response-column {
    flex: 1;
    min-width: 0;
  }

  section {
    margin-top: 1rem;
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .environment {
    flex-direction: row;
    align-items: center;
    gap: 0.75rem;
    font-size: 0.9rem;
    margin-top: 1rem;
  }

  .environment details {
    margin-left: auto;
  }

  .environment ul {
    margin: 0.25rem 0 0;
    padding-left: 1.25rem;
  }

  .auth-fields {
    display: flex;
    gap: 0.5rem;
  }

  .auth-fields input {
    flex: 1;
  }

  .hint {
    margin: 0.15rem 0 0;
    font-size: 0.8rem;
    opacity: 0.7;
  }

  textarea,
  input,
  select,
  button {
    font-family: inherit;
    font-size: 0.95rem;
    padding: 0.5rem;
    border-radius: var(--radius);
    border: 1px solid var(--border);
    color: var(--text);
    background: var(--bg-elevated);
  }

  button {
    cursor: pointer;
  }

  button:disabled {
    opacity: 0.55;
    cursor: default;
  }

  .error {
    color: var(--danger);
  }

  .sub-tabs {
    display: flex;
    gap: 0.4rem;
    margin-bottom: 0.4rem;
  }

  .sub-tabs button {
    background: transparent;
    font-size: 0.85rem;
    padding: 0.3rem 0.6rem;
  }

  .sub-tabs button.active {
    background: var(--accent);
    color: var(--accent-text);
    border-color: var(--accent);
  }

  .script {
    font-family: ui-monospace, "SF Mono", Consolas, monospace;
    font-size: 0.85rem;
  }

  .script-error-hint {
    color: var(--danger);
    font-size: 0.9rem;
  }
</style>
