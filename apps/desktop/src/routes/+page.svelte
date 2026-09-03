<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { open } from "@tauri-apps/plugin-dialog";
  import CollectionTree, { type TreeNode } from "$lib/CollectionTree.svelte";
  import ResponsePanel from "$lib/ResponsePanel.svelte";
  import type { SendResponseResult } from "$lib/types";

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
  };

  type CollectionSummary = {
    name: string;
    root: string;
    items: TreeNode[];
    environments: { name: string; path: string }[];
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
    sending: boolean;
    saving: boolean;
    error: string;
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
      sending: false,
      saving: false,
      error: "",
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
        sending: false,
        saving: false,
        error: "",
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
    try {
      await invoke("save_request", {
        path: tab.filePath,
        method: tab.method,
        url: tab.url,
        headers: parseHeaders(tab.headersText),
        auth: authPayload(tab),
        body: tab.body || null,
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
    tab.response = null;
    try {
      tab.response = await invoke<SendResponseResult>("send_request", {
        requestKey: requestKeyOf(tab),
        requestLabel: tab.title,
        method: tab.method,
        url: tab.url,
        headers: parseHeaders(tab.headersText),
        body: tab.body || null,
        auth: authPayload(tab),
      });
    } catch (e) {
      tab.error = String(e);
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
  <aside class="sidebar">
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
      <button type="button" onclick={openCollection}>Open Collection...</button>
    {/if}
    {#if sidebarError}
      <p class="error">{sidebarError}</p>
    {/if}
  </aside>

  <main>
    <h1>FluxChunk</h1>

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

      <section>
        <label for="headers">Headers (one per line, "Key: Value")</label>
        <textarea id="headers" bind:value={tab.headersText} rows="3"></textarea>
      </section>

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

      <section>
        <label for="body">Body</label>
        <textarea id="body" bind:value={tab.body} rows="5" placeholder="(optional)"></textarea>
      </section>

      {#if tab.error}
        <p class="error">{tab.error}</p>
      {/if}

      {#if tab.response}
        <ResponsePanel response={tab.response} requestKey={requestKeyOf(tab)} />
      {/if}
    {/if}
  </main>
</div>

<style>
  :root {
    font-family: Inter, Avenir, Helvetica, Arial, sans-serif;
    color: #0f0f0f;
    background-color: #f6f6f6;
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
    border-right: 1px solid rgba(127, 127, 127, 0.25);
    min-height: 100vh;
  }

  .sidebar-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    font-size: 0.9rem;
    margin-bottom: 0.5rem;
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

  .tab-strip {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    flex-wrap: wrap;
    margin-bottom: 0.75rem;
    border-bottom: 1px solid rgba(127, 127, 127, 0.25);
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
    background: rgba(127, 127, 127, 0.12);
    border-color: rgba(127, 127, 127, 0.25);
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
    color: #d18b00;
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
    border-radius: 6px;
    border: 1px solid #ccc;
  }

  button {
    cursor: pointer;
    background: #ffffff;
  }

  button:disabled {
    opacity: 0.55;
    cursor: default;
  }

  .error {
    color: #b00020;
  }

  @media (prefers-color-scheme: dark) {
    :root {
      color: #f6f6f6;
      background-color: #2f2f2f;
    }

    .sidebar {
      border-right-color: rgba(255, 255, 255, 0.15);
    }

    .tab-strip {
      border-bottom-color: rgba(255, 255, 255, 0.15);
    }

    textarea,
    input,
    select,
    button {
      color: #f6f6f6;
      background-color: #0f0f0f98;
      border-color: #444;
    }
  }
</style>
