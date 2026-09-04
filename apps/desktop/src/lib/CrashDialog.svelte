<script lang="ts" module>
  export type CrashInfo = { path: string; summary: string };
</script>

<script lang="ts">
  // Spec section 16, step 2-4: "calm 'app closed unexpectedly' notice...
  // never alarming", with "View details", "Report this issue" (a
  // pre-filled GitHub new-issue URL -- no API, no token, no backend;
  // person reviews and edits before submitting on GitHub's own page),
  // and a "copy report" fallback. Nothing is ever sent automatically.

  const REPO = "v8integra/FluxChunk";

  let { crash, onLoadDetails, onDismiss }: { crash: CrashInfo; onLoadDetails: () => Promise<string>; onDismiss: () => void } = $props();

  let details = $state<string | null>(null);
  let detailsError = $state("");
  let loadingDetails = $state(false);
  let copied = $state(false);

  async function viewDetails() {
    loadingDetails = true;
    detailsError = "";
    try {
      details = await onLoadDetails();
    } catch (e) {
      detailsError = String(e);
    } finally {
      loadingDetails = false;
    }
  }

  async function reportIssue() {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    const title = `Crash: ${crash.summary}`;
    const body = [
      "<!-- Feel free to add anything else about what you were doing. -->",
      "",
      "```",
      details ?? crash.summary,
      "```",
    ].join("\n");
    const url = `https://github.com/${REPO}/issues/new?title=${encodeURIComponent(title)}&body=${encodeURIComponent(body)}`;
    await openUrl(url);
  }

  async function copyReport() {
    const text = details ?? crash.summary;
    await navigator.clipboard.writeText(text);
    copied = true;
    setTimeout(() => (copied = false), 2000);
  }
</script>

<div class="overlay">
  <div class="dialog" role="dialog" aria-modal="true">
    <h2>FluxChunk closed unexpectedly</h2>
    <p>It looks like the app didn't shut down cleanly last time. No data was sent anywhere -- everything below stays on this machine unless you choose to report it.</p>

    {#if !details}
      <button type="button" onclick={viewDetails} disabled={loadingDetails}>{loadingDetails ? "Loading..." : "View details"}</button>
      {#if detailsError}
        <p class="error">{detailsError}</p>
      {/if}
    {:else}
      <pre class="details">{details}</pre>
    {/if}

    <div class="actions">
      <button type="button" onclick={onDismiss}>Dismiss</button>
      <button type="button" onclick={copyReport}>{copied ? "Copied!" : "Copy report"}</button>
      <button type="button" class="primary" onclick={reportIssue}>Report this issue</button>
    </div>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }

  .dialog {
    background: var(--bg-elevated);
    color: var(--text);
    border-radius: var(--radius);
    border: 1px solid var(--border);
    padding: 1.25rem;
    max-width: 32rem;
    width: 90%;
    max-height: 80vh;
    overflow: auto;
  }

  .dialog h2 {
    margin-top: 0;
  }

  .details {
    white-space: pre-wrap;
    word-break: break-word;
    background: var(--bg-hover);
    border-radius: 6px;
    padding: 0.6rem 0.75rem;
    font-size: 0.78rem;
    max-height: 40vh;
    overflow: auto;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
    margin-top: 1rem;
  }

  .primary {
    background: var(--accent);
    color: var(--accent-text);
    border-color: var(--accent);
  }

  .error {
    color: var(--danger);
  }
</style>
