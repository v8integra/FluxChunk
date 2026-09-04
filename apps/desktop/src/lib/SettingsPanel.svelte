<script lang="ts">
  // Update preferences (spec section 13) don't have a natural home among
  // the always-visible toolbar controls the way theme/layout do -- this
  // is the app's first real "Settings" surface, and the obvious place to
  // grow the rest of spec section 11's tier-1 settings (update-check
  // preference is the only one of that list actually implemented so far)
  // once there's something else to put here.

  let {
    autoCheckUpdates,
    updateCheckUrl,
    verboseLogging,
    onChange,
    onVerboseLoggingChange,
    onClose,
    onCheckNow,
  }: {
    autoCheckUpdates: boolean;
    updateCheckUrl: string;
    verboseLogging: boolean;
    onChange: (autoCheckUpdates: boolean, updateCheckUrl: string) => void;
    onVerboseLoggingChange: (verboseLogging: boolean) => void;
    onClose: () => void;
    onCheckNow: () => void;
  } = $props();

  let localAuto = $state(autoCheckUpdates);
  let localUrl = $state(updateCheckUrl);
  let localVerbose = $state(verboseLogging);

  function toggleAuto() {
    localAuto = !localAuto;
    onChange(localAuto, localUrl);
  }

  function commitUrl() {
    onChange(localAuto, localUrl);
  }

  // Spec section 16: "Explicit 'Verbose logging' toggle for bodies, with
  // a one-time warning before enabling." Turning it off never needs a
  // warning -- only the transition into logging bodies does.
  function toggleVerbose() {
    if (!localVerbose) {
      const confirmed = confirm(
        "Verbose logging writes full request/response bodies to the local log file (still never vault secrets). Enable it?",
      );
      if (!confirmed) return;
    }
    localVerbose = !localVerbose;
    onVerboseLoggingChange(localVerbose);
  }
</script>

<div class="overlay">
  <div class="dialog" role="dialog" aria-modal="true">
    <div class="header">
      <h2>Settings</h2>
      <button type="button" class="close" onclick={onClose} aria-label="Close">&times;</button>
    </div>

    <section>
      <h3>Updates</h3>
      <label class="checkbox-row">
        <input type="checkbox" checked={localAuto} onchange={toggleAuto} />
        Automatically check for updates on launch
      </label>
      <p class="hint">Off by default -- checking is never automatic unless you turn this on. The manual "Check for Updates" button always works either way.</p>

      <label for="update-url">Update check URL (advanced)</label>
      <input id="update-url" type="text" placeholder="Leave blank to use the default public feed" bind:value={localUrl} onblur={commitUrl} />
      <p class="hint">Point this at an internally hosted manifest instead of the public GitHub feed -- useful for an air-gapped or IT-managed install.</p>

      <button type="button" onclick={onCheckNow}>Check for Updates now</button>
    </section>

    <section>
      <h3>Logging</h3>
      <label class="checkbox-row">
        <input type="checkbox" checked={localVerbose} onchange={toggleVerbose} />
        Verbose logging (include request/response bodies)
      </label>
      <p class="hint">
        Off by default -- normal logs only ever record method/host/status/timing. Vault secrets are never logged, even with this on.
        Logs live locally under FluxChunk's app data folder and are purged after 7 days or 50MB, whichever comes first.
      </p>
    </section>
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

  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .header h2 {
    margin: 0;
  }

  .close {
    background: transparent;
    border: none;
    font-size: 1.1rem;
    padding: 0.1rem 0.4rem;
  }

  h3 {
    font-size: 0.9rem;
    margin: 1rem 0 0.5rem;
  }

  .checkbox-row {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.88rem;
  }

  .checkbox-row input {
    width: auto;
  }

  label {
    display: block;
    font-size: 0.85rem;
    margin-top: 0.75rem;
  }

  input[type="text"] {
    width: 100%;
    box-sizing: border-box;
    margin-top: 0.25rem;
  }

  .hint {
    margin: 0.25rem 0 0;
    font-size: 0.78rem;
    opacity: 0.7;
  }

  button {
    margin-top: 0.9rem;
  }
</style>
