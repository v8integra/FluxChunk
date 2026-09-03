<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import JsonTree from "./JsonTree.svelte";
  import DiffTree from "./DiffTree.svelte";
  import { findJsonMatches, pathKey, ancestorPathKeys } from "./jsonSearch";
  import type { SendResponseResult, HistoryEntrySummary, HistoryEntryDetail, DiffNode, BodyPreview } from "./types";

  type SubTab = "pretty" | "raw" | "preview" | "headers" | "cookies" | "history";
  const subTabs: SubTab[] = ["pretty", "raw", "preview", "headers", "cookies", "history"];

  let { response, requestKey }: { response: SendResponseResult; requestKey: string } = $props();

  let subTab = $state<SubTab>("pretty");
  let searchQuery = $state("");
  let searchMatchIndex = $state(0);

  let fullBody = $state<BodyPreview | null>(null);
  let loadingFull = $state(false);
  let loadFullError = $state("");
  let effectiveBody = $derived(fullBody ?? response.body);

  let matches = $derived(
    effectiveBody.kind === "json" && effectiveBody.json !== null ? findJsonMatches(effectiveBody.json, searchQuery) : [],
  );
  let matchSet = $derived(new Set(matches.map((m) => pathKey(m))));
  let expandSet = $derived(new Set(matches.flatMap((m) => ancestorPathKeys(m))));
  let currentMatchKey = $derived(matches.length > 0 ? pathKey(matches[searchMatchIndex % matches.length]) : null);

  // Reset per-response UI state whenever a new response arrives (new
  // send, or the tab switched to a different response object).
  $effect(() => {
    void response;
    fullBody = null;
    loadFullError = "";
    searchQuery = "";
    searchMatchIndex = 0;
    historyEntries = [];
    historyLoaded = false;
    selectedEntry = null;
    diffResult = null;
    diffError = "";
  });

  $effect(() => {
    if (!currentMatchKey) return;
    const el = document.querySelector(`[data-json-path="${CSS.escape(currentMatchKey)}"]`);
    el?.scrollIntoView({ block: "center", behavior: "smooth" });
  });

  async function loadFull() {
    loadingFull = true;
    loadFullError = "";
    try {
      fullBody = await invoke<BodyPreview>("load_full_response_body", { historyId: response.history_id });
    } catch (e) {
      loadFullError = String(e);
    } finally {
      loadingFull = false;
    }
  }

  function nextMatch() {
    if (matches.length === 0) return;
    searchMatchIndex = (searchMatchIndex + 1) % matches.length;
  }
  function prevMatch() {
    if (matches.length === 0) return;
    searchMatchIndex = (searchMatchIndex - 1 + matches.length) % matches.length;
  }

  // --- history ---
  let historyEntries = $state<HistoryEntrySummary[]>([]);
  let historyLoaded = $state(false);
  let historyLoading = $state(false);
  let historyError = $state("");
  let selectedEntry = $state<HistoryEntryDetail | null>(null);
  let diffResult = $state<DiffNode | null>(null);
  let diffError = $state("");
  let diffing = $state(false);

  async function loadHistory() {
    historyLoading = true;
    historyError = "";
    try {
      historyEntries = await invoke<HistoryEntrySummary[]>("list_history", { requestKey });
      historyLoaded = true;
    } catch (e) {
      historyError = String(e);
    } finally {
      historyLoading = false;
    }
  }

  async function selectSubTab(t: SubTab) {
    subTab = t;
    if (t === "history" && !historyLoaded) await loadHistory();
  }

  async function viewEntry(id: number) {
    historyError = "";
    diffResult = null;
    try {
      selectedEntry = await invoke<HistoryEntryDetail>("get_history_entry", { id });
    } catch (e) {
      historyError = String(e);
    }
  }

  async function compareEntries(a: number, b: number) {
    diffing = true;
    diffError = "";
    try {
      diffResult = await invoke<DiffNode>("diff_history", { a, b });
    } catch (e) {
      diffError = String(e);
    } finally {
      diffing = false;
    }
  }

  async function clearHistoryFn() {
    if (!confirm("Clear response history for this request?")) return;
    try {
      await invoke("clear_history", { requestKey });
      historyEntries = [];
      selectedEntry = null;
      diffResult = null;
    } catch (e) {
      historyError = String(e);
    }
  }

  function formatTime(ms: number): string {
    return new Date(ms).toLocaleString();
  }

  function formatSize(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
  }

  function capitalize(s: string): string {
    return s[0].toUpperCase() + s.slice(1);
  }
</script>

<section class="response-panel">
  <h2>{response.status} {response.status_text} &middot; {response.elapsed_ms}ms</h2>
  <p class="resolved-url">{response.resolved_url}</p>

  <div class="sub-tabs">
    {#each subTabs as t (t)}
      <button type="button" class:active={subTab === t} onclick={() => selectSubTab(t)}>{capitalize(t)}</button>
    {/each}
  </div>

  {#if subTab === "pretty" || subTab === "raw"}
    {#if effectiveBody.exceeds_threshold}
      <div class="gate">
        <p>Response is {formatSize(effectiveBody.size_bytes)} -- not rendered automatically past 5&nbsp;MB.</p>
        <button type="button" onclick={loadFull} disabled={loadingFull}>{loadingFull ? "Loading..." : "Load full response"}</button>
        {#if loadFullError}<p class="error">{loadFullError}</p>{/if}
      </div>
    {:else if subTab === "pretty" && effectiveBody.kind === "json" && effectiveBody.json !== null}
      <div class="search-bar">
        <input type="text" placeholder="Search keys/values..." bind:value={searchQuery} />
        {#if searchQuery}
          <span class="match-count">{matches.length > 0 ? `${(searchMatchIndex % matches.length) + 1} / ${matches.length}` : "0 / 0"}</span>
          <button type="button" onclick={prevMatch} disabled={matches.length === 0} aria-label="Previous match">&uarr;</button>
          <button type="button" onclick={nextMatch} disabled={matches.length === 0} aria-label="Next match">&darr;</button>
        {/if}
      </div>
      <div class="json-tree-container">
        <JsonTree value={effectiveBody.json} {matchSet} {expandSet} {currentMatchKey} />
      </div>
    {:else if subTab === "raw"}
      <pre>{effectiveBody.kind === "json" && effectiveBody.json !== null
          ? JSON.stringify(effectiveBody.json, null, 2)
          : (effectiveBody.text ?? "(no text content)")}</pre>
    {:else if effectiveBody.text !== null}
      <pre>{effectiveBody.text}</pre>
    {:else}
      <p class="hint">No preview available for content type "{effectiveBody.content_type ?? "unknown"}".</p>
    {/if}
  {:else if subTab === "preview"}
    {#if response.body.kind === "html" && response.body.text !== null}
      <iframe title="Response HTML preview" class="preview-frame" sandbox="" srcdoc={response.body.text}></iframe>
    {:else}
      <p class="hint">No preview available -- only HTML responses render here.</p>
    {/if}
  {:else if subTab === "headers"}
    <table class="kv-table">
      <tbody>
        {#each Object.entries(response.headers) as [k, v] (k)}
          <tr>
            <td>{k}</td>
            <td>{v}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {:else if subTab === "cookies"}
    {#if response.cookies.length === 0}
      <p class="hint">No cookies set.</p>
    {:else}
      <table class="kv-table cookies">
        <thead>
          <tr>
            <th>Name</th>
            <th>Value</th>
            <th>Domain</th>
            <th>Path</th>
            <th>Expires</th>
            <th>Flags</th>
          </tr>
        </thead>
        <tbody>
          {#each response.cookies as c (c.name + c.value)}
            <tr>
              <td>{c.name}</td>
              <td>{c.value}</td>
              <td>{c.domain ?? "-"}</td>
              <td>{c.path ?? "-"}</td>
              <td>{c.expires ?? c.max_age ?? "-"}</td>
              <td>{[c.secure ? "Secure" : "", c.http_only ? "HttpOnly" : "", c.same_site ?? ""].filter(Boolean).join(", ") || "-"}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  {:else if subTab === "history"}
    <div class="history-panel">
      <div class="history-header">
        <button type="button" onclick={loadHistory} disabled={historyLoading}>{historyLoading ? "Loading..." : "Refresh"}</button>
        <button type="button" onclick={clearHistoryFn} disabled={historyEntries.length === 0}>Clear history</button>
      </div>
      {#if historyError}<p class="error">{historyError}</p>{/if}
      {#if historyLoaded && historyEntries.length === 0}
        <p class="hint">No past runs recorded yet.</p>
      {/if}
      <ul class="history-list">
        {#each historyEntries as entry (entry.id)}
          <li>
            <button type="button" class="history-entry" class:selected={selectedEntry?.id === entry.id} onclick={() => viewEntry(entry.id)}>
              <span class="hist-status" class:ok={entry.status < 400}>{entry.status}</span>
              <span>{entry.method}</span>
              <span class="hist-time">{formatTime(entry.sent_at)}</span>
              <span class="hist-size">{formatSize(entry.size_bytes)}</span>
            </button>
            <button
              type="button"
              class="compare-btn"
              title="Compare with the live response above"
              onclick={() => compareEntries(response.history_id, entry.id)}>Diff vs live</button
            >
          </li>
        {/each}
      </ul>

      {#if selectedEntry}
        <div class="history-detail">
          <h3>{selectedEntry.status} {selectedEntry.status_text} &middot; {selectedEntry.elapsed_ms}ms</h3>
          {#if selectedEntry.body.kind === "json" && selectedEntry.body.json !== null}
            <div class="json-tree-container">
              <JsonTree value={selectedEntry.body.json} matchSet={new Set()} expandSet={new Set()} currentMatchKey={null} />
            </div>
          {:else}
            <pre>{selectedEntry.body.text ?? "(no text content)"}</pre>
          {/if}
          {#if historyEntries.length > 1}
            <div class="compare-row">
              <span class="hint">Compare with:</span>
              {#each historyEntries.filter((e) => e.id !== selectedEntry?.id) as candidate (candidate.id)}
                <button type="button" onclick={() => compareEntries(selectedEntry!.id, candidate.id)}>{formatTime(candidate.sent_at)}</button>
              {/each}
            </div>
          {/if}
        </div>
      {/if}

      {#if diffing}<p class="hint">Diffing...</p>{/if}
      {#if diffError}<p class="error">{diffError}</p>{/if}
      {#if diffResult}
        <div class="diff-container">
          <DiffTree node={diffResult} />
        </div>
      {/if}
    </div>
  {/if}
</section>

<style>
  .response-panel {
    margin-top: 1rem;
  }

  .resolved-url {
    margin: 0 0 0.5rem;
    font-size: 0.85rem;
    opacity: 0.7;
    word-break: break-all;
  }

  .sub-tabs {
    display: flex;
    gap: 0.25rem;
    border-bottom: 1px solid var(--border);
    margin-bottom: 0.6rem;
    padding-bottom: 0.3rem;
  }

  .sub-tabs button {
    font-size: 0.85rem;
    padding: 0.25rem 0.6rem;
    background: transparent;
  }

  .sub-tabs button.active {
    background: var(--bg-hover);
    font-weight: 600;
  }

  .gate {
    padding: 1rem;
    border: 1px dashed var(--border);
    border-radius: 6px;
    text-align: center;
  }

  .search-bar {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    margin-bottom: 0.5rem;
  }

  .search-bar input {
    flex: 1;
    max-width: 20rem;
  }

  .match-count {
    font-size: 0.8rem;
    opacity: 0.7;
    white-space: nowrap;
  }

  .json-tree-container {
    max-height: 55vh;
    overflow: auto;
    background: var(--bg-hover);
    border-radius: 6px;
    padding: 0.6rem;
  }

  pre {
    white-space: pre-wrap;
    word-break: break-word;
    background: var(--bg-hover);
    border-radius: 6px;
    padding: 0.75rem;
    max-height: 55vh;
    overflow: auto;
  }

  .preview-frame {
    width: 100%;
    height: 55vh;
    border: 1px solid var(--border);
    border-radius: 6px;
    /* Deliberately always white, not theme-driven: this renders arbitrary
       external HTML, which expects a normal page background regardless
       of the app's own theme. */
    background: #fff;
  }

  .kv-table {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.85rem;
  }

  .kv-table td,
  .kv-table th {
    text-align: left;
    padding: 0.3rem 0.5rem;
    border-bottom: 1px solid var(--border);
    word-break: break-word;
  }

  .kv-table td:first-child {
    font-weight: 600;
    white-space: nowrap;
    width: 1%;
  }

  .hint {
    font-size: 0.85rem;
    opacity: 0.7;
  }

  .error {
    color: var(--danger);
  }

  .history-header {
    display: flex;
    gap: 0.5rem;
    margin-bottom: 0.5rem;
  }

  .history-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
  }

  .history-list li {
    display: flex;
    align-items: center;
    gap: 0.3rem;
  }

  .history-entry {
    flex: 1;
    display: flex;
    gap: 0.6rem;
    align-items: center;
    text-align: left;
    font-size: 0.82rem;
    padding: 0.3rem 0.5rem;
    background: transparent;
  }

  .history-entry.selected {
    background: var(--bg-hover);
    box-shadow: inset 2px 0 0 var(--accent);
  }

  .hist-status {
    font-weight: 700;
    color: var(--danger);
  }

  .hist-status.ok {
    color: var(--success);
  }

  .hist-time,
  .hist-size {
    opacity: 0.7;
    margin-left: auto;
  }

  .compare-btn {
    font-size: 0.75rem;
    padding: 0.2rem 0.4rem;
    white-space: nowrap;
  }

  .history-detail {
    margin-top: 0.75rem;
    padding-top: 0.5rem;
    border-top: 1px solid var(--border);
  }

  .compare-row {
    display: flex;
    flex-wrap: wrap;
    gap: 0.3rem;
    align-items: center;
    margin-top: 0.5rem;
  }

  .compare-row button {
    font-size: 0.78rem;
    padding: 0.2rem 0.45rem;
  }

  .diff-container {
    margin-top: 0.75rem;
    background: var(--bg-hover);
    border-radius: 6px;
    padding: 0.6rem;
    max-height: 50vh;
    overflow: auto;
  }
</style>
