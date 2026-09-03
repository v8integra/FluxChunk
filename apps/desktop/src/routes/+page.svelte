<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { open } from "@tauri-apps/plugin-dialog";

  type SendResponseResult = {
    status: number;
    status_text: string;
    headers: Record<string, string>;
    body: string;
    elapsed_ms: number;
    resolved_url: string;
  };

  type EnvironmentSummary = {
    name: string;
    vars: Record<string, string>;
  };

  type AuthMode = "none" | "inherit" | "basic" | "bearer" | "apikey" | "oauth2";

  let method = $state("GET");
  let url = $state("https://api.wheretheiss.at/v1/satellites/25544");
  let headersText = $state("Accept: application/json");
  let body = $state("");
  let sending = $state(false);
  let error = $state("");
  let response = $state<SendResponseResult | null>(null);

  let environment = $state<EnvironmentSummary | null>(null);
  let environmentError = $state("");

  let authMode = $state<AuthMode>("none");
  let authUsername = $state("");
  let authPassword = $state("");
  let authToken = $state("");
  let authApiKeyName = $state("");
  let authApiKeyValue = $state("");
  let authApiKeyPlacement = $state("header");
  let authAccessToken = $state("");

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

  function authPayload() {
    return {
      mode: authMode,
      username: authUsername || null,
      password: authPassword || null,
      token: authToken || null,
      key: authApiKeyName || null,
      value: authApiKeyValue || null,
      placement: authApiKeyPlacement,
      accessToken: authAccessToken || null,
    };
  }

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

  async function send(event: Event) {
    event.preventDefault();
    sending = true;
    error = "";
    response = null;
    try {
      response = await invoke<SendResponseResult>("send_request", {
        method,
        url,
        headers: parseHeaders(headersText),
        body: body || null,
        auth: authPayload(),
      });
    } catch (e) {
      error = String(e);
    } finally {
      sending = false;
    }
  }
</script>

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
            {#each Object.entries(environment.vars) as [key, value]}
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

  <form onsubmit={send} class="request-row">
    <select bind:value={method}>
      <option>GET</option>
      <option>POST</option>
      <option>PUT</option>
      <option>PATCH</option>
      <option>DELETE</option>
      <option>HEAD</option>
      <option>OPTIONS</option>
    </select>
    <input class="url" bind:value={url} placeholder="https://api.example.com/..." />
    <button type="submit" disabled={sending}>{sending ? "Sending..." : "Send"}</button>
  </form>

  <section>
    <label for="headers">Headers (one per line, "Key: Value")</label>
    <textarea id="headers" bind:value={headersText} rows="3"></textarea>
  </section>

  <section class="auth">
    <label for="auth-mode">Auth</label>
    <select id="auth-mode" bind:value={authMode}>
      <option value="none">None</option>
      <option value="inherit">Inherit</option>
      <option value="basic">Basic</option>
      <option value="bearer">Bearer</option>
      <option value="apikey">API Key</option>
      <option value="oauth2">OAuth2</option>
    </select>

    {#if authMode === "inherit"}
      <p class="hint">No collection to inherit from yet -- resolves the same as None.</p>
    {:else if authMode === "basic"}
      <div class="auth-fields">
        <input bind:value={authUsername} placeholder="Username" />
        <input bind:value={authPassword} type="password" placeholder="Password" />
      </div>
    {:else if authMode === "bearer"}
      <div class="auth-fields">
        <input bind:value={authToken} type="password" placeholder="Token" />
      </div>
    {:else if authMode === "apikey"}
      <div class="auth-fields">
        <input bind:value={authApiKeyName} placeholder="Key name (e.g. X-API-Key)" />
        <input bind:value={authApiKeyValue} type="password" placeholder="Value" />
        <select bind:value={authApiKeyPlacement}>
          <option value="header">Header</option>
          <option value="query">Query param</option>
        </select>
      </div>
    {:else if authMode === "oauth2"}
      <div class="auth-fields">
        <input bind:value={authAccessToken} type="password" placeholder="Access token" />
      </div>
      <p class="hint">Paste a token you already have -- interactive login isn't implemented yet.</p>
    {/if}
    <p class="hint">Auth values may use {'{{var}}'} / {'{{vault:...}}'} too, same as headers.</p>
  </section>

  <section>
    <label for="body">Body</label>
    <textarea id="body" bind:value={body} rows="5" placeholder="(optional)"></textarea>
  </section>

  {#if error}
    <p class="error">{error}</p>
  {/if}

  {#if response}
    <section class="response">
      <h2>{response.status} {response.status_text} &middot; {response.elapsed_ms}ms</h2>
      <p class="resolved-url">{response.resolved_url}</p>
      <pre>{response.body}</pre>
    </section>
  {/if}
</main>

<style>
  :root {
    font-family: Inter, Avenir, Helvetica, Arial, sans-serif;
    color: #0f0f0f;
    background-color: #f6f6f6;
  }

  main {
    max-width: 900px;
    margin: 0 auto;
    padding: 2rem 1.5rem;
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

  .resolved-url {
    margin: 0 0 0.5rem;
    font-size: 0.85rem;
    opacity: 0.7;
    word-break: break-all;
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

  pre {
    white-space: pre-wrap;
    word-break: break-word;
    background: #fff;
    border: 1px solid #ddd;
    border-radius: 6px;
    padding: 0.75rem;
    max-height: 50vh;
    overflow: auto;
  }

  .error {
    color: #b00020;
  }

  @media (prefers-color-scheme: dark) {
    :root {
      color: #f6f6f6;
      background-color: #2f2f2f;
    }

    textarea,
    input,
    select,
    button,
    pre {
      color: #f6f6f6;
      background-color: #0f0f0f98;
      border-color: #444;
    }
  }
</style>
