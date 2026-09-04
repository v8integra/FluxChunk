<script lang="ts" module>
  export type UpdateInfo = { version: string; notes: string | null; pub_date: string | null };
  export type UpdateStage = "checking" | "up-to-date" | "available" | "downloading" | "ready" | "error";
</script>

<script lang="ts">
  // Spec section 13's flow, exactly: Check -> "Update available" dialog
  // with changelog -> Approve & Download -> separate Install and Restart
  // action. "Rejecting just skips; can check again anytime" -- Not now /
  // Later both just close this dialog, no backend call.

  let {
    stage,
    info,
    error,
    onApproveDownload,
    onInstallAndRestart,
    onDismiss,
  }: {
    stage: UpdateStage;
    info: UpdateInfo | null;
    error: string;
    onApproveDownload: () => void;
    onInstallAndRestart: () => void;
    onDismiss: () => void;
  } = $props();
</script>

<div class="overlay">
  <div class="dialog" role="dialog" aria-modal="true">
    {#if stage === "checking"}
      <h2>Checking for updates...</h2>
    {:else if stage === "up-to-date"}
      <h2>You're up to date</h2>
      <div class="actions">
        <button type="button" class="primary" onclick={onDismiss}>OK</button>
      </div>
    {:else if stage === "error"}
      <h2>Couldn't check for updates</h2>
      <p class="error">{error}</p>
      <div class="actions">
        <button type="button" class="primary" onclick={onDismiss}>OK</button>
      </div>
    {:else if stage === "available" && info}
      <h2>Update available: {info.version}</h2>
      {#if info.notes}
        <pre class="notes">{info.notes}</pre>
      {/if}
      <div class="actions">
        <button type="button" onclick={onDismiss}>Not now</button>
        <button type="button" class="primary" onclick={onApproveDownload}>Approve &amp; Download</button>
      </div>
    {:else if stage === "downloading"}
      <h2>Downloading update...</h2>
    {:else if stage === "ready"}
      <h2>Ready to install</h2>
      <p>Version {info?.version} has been downloaded. Installing will close FluxChunk and restart it.</p>
      {#if error}
        <p class="error">{error}</p>
      {/if}
      <div class="actions">
        <button type="button" onclick={onDismiss}>Later</button>
        <button type="button" class="primary" onclick={onInstallAndRestart}>Install and Restart</button>
      </div>
    {/if}
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
    max-width: 28rem;
    width: 90%;
    max-height: 80vh;
    overflow: auto;
  }

  .dialog h2 {
    margin-top: 0;
  }

  .notes {
    white-space: pre-wrap;
    word-break: break-word;
    background: var(--bg-hover);
    border-radius: 6px;
    padding: 0.6rem 0.75rem;
    font-size: 0.85rem;
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
