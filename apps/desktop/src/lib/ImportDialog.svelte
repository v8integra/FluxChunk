<script lang="ts" module>
  export type FindingDto = { severity: string; rule: string; message: string; snippet: string };
  export type RequestFindingsDto = { request_name: string; findings: FindingDto[] };
  export type ImportPreviewDto = {
    name: string;
    request_count: number;
    parse_warnings: string[];
    security_findings: RequestFindingsDto[];
  };
</script>

<script lang="ts">
  // Spec section 8's two-dialog flow: an import summary (Cancel / Scan &
  // Continue), then -- only if the scan actually found something --
  // security findings (Reject Import / Import & Skip Flagged Scripts /
  // Import Anyway). The scan itself already ran by the time this dialog
  // opens (see ResponsePanel-style backend calls in +page.svelte), so
  // "Scan & Continue" here just reveals results that already exist;
  // behaviorally indistinguishable from triggering the scan on click.

  let {
    stage,
    preview,
    committing,
    error,
    onCancel,
    onScanContinue,
    onRejectImport,
    onImportSkipFlagged,
    onImportAnyway,
  }: {
    stage: "summary" | "findings";
    preview: ImportPreviewDto;
    committing: boolean;
    error: string;
    onCancel: () => void;
    onScanContinue: () => void;
    onRejectImport: () => void;
    onImportSkipFlagged: () => void;
    onImportAnyway: () => void;
  } = $props();

  let totalFindings = $derived(preview.security_findings.reduce((n, r) => n + r.findings.length, 0));
</script>

<div class="overlay">
  <div class="dialog" role="dialog" aria-modal="true">
    {#if stage === "summary"}
      <h2>Import &quot;{preview.name}&quot;</h2>
      <p>{preview.request_count} request(s) will be imported.</p>
      {#if preview.parse_warnings.length > 0}
        <details open>
          <summary>{preview.parse_warnings.length} note(s)</summary>
          <ul>
            {#each preview.parse_warnings as w, i (i)}
              <li>{w}</li>
            {/each}
          </ul>
        </details>
      {/if}
      <div class="actions">
        <button type="button" onclick={onCancel} disabled={committing}>Cancel</button>
        <button type="button" class="primary" onclick={onScanContinue} disabled={committing}>
          {#if committing}
            Importing...
          {:else if totalFindings > 0}
            Scan &amp; Continue
          {:else}
            Import
          {/if}
        </button>
      </div>
    {:else}
      <h2>Security findings</h2>
      <p class="hint">
        {totalFindings} issue(s) found across {preview.security_findings.length} request(s). Fixed heuristics only -- not a guarantee
        something flagged is actually harmful, or that nothing else is.
      </p>
      <div class="findings-list">
        {#each preview.security_findings as rf (rf.request_name)}
          <div class="request-findings">
            <strong>{rf.request_name}</strong>
            {#each rf.findings as f, i (i)}
              <div class="finding severity-{f.severity}">
                <span class="badge">{f.severity}</span>
                <p class="message">{f.message}</p>
                <pre class="snippet">{f.snippet}</pre>
              </div>
            {/each}
          </div>
        {/each}
      </div>
      {#if error}
        <p class="error">{error}</p>
      {/if}
      <div class="actions">
        <button type="button" class="primary" onclick={onRejectImport} disabled={committing}>Reject Import</button>
        <button type="button" onclick={onImportSkipFlagged} disabled={committing}>
          {committing ? "Importing..." : "Import & Skip Flagged Scripts"}
        </button>
        <button type="button" class="override" onclick={onImportAnyway} disabled={committing}>
          {committing ? "Importing..." : "Import Anyway"}
        </button>
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
    max-width: 32rem;
    width: 90%;
    max-height: 80vh;
    overflow: auto;
  }

  .dialog h2 {
    margin-top: 0;
  }

  .hint {
    font-size: 0.85rem;
    opacity: 0.75;
  }

  .findings-list {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    margin: 0.75rem 0;
  }

  .request-findings {
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 0.5rem 0.75rem;
  }

  .finding {
    margin-top: 0.4rem;
    padding: 0.4rem 0.5rem;
    border-radius: 4px;
  }

  .finding.severity-critical {
    background: rgba(248, 81, 73, 0.12);
  }

  .finding.severity-warning {
    background: rgba(210, 153, 34, 0.12);
  }

  .badge {
    font-size: 0.7rem;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }

  .severity-critical .badge {
    color: var(--danger);
  }

  .severity-warning .badge {
    color: var(--warning);
  }

  .message {
    margin: 0.2rem 0;
    font-size: 0.85rem;
  }

  .snippet {
    margin: 0;
    font-size: 0.78rem;
    background: var(--bg-hover);
    padding: 0.3rem 0.5rem;
    border-radius: 4px;
    overflow-x: auto;
    white-space: pre-wrap;
    word-break: break-word;
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

  /* "Import Anyway" is deliberately the least visually prominent option
     here -- spec section 8 wants it styled as a conscious override, not
     the default action a hurried click would land on. */
  .override {
    background: transparent;
    border-color: transparent;
    color: var(--text-muted);
    font-size: 0.85rem;
  }

  .error {
    color: var(--danger);
  }
</style>
