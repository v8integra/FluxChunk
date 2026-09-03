<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  type SendResponseResult = {
    status: number;
    status_text: string;
    headers: Record<string, string>;
    body: string;
    elapsed_ms: number;
  };

  let method = $state("GET");
  let url = $state("https://api.wheretheiss.at/v1/satellites/25544");
  let headersText = $state("Accept: application/json");
  let body = $state("");
  let sending = $state(false);
  let error = $state("");
  let response = $state<SendResponseResult | null>(null);

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
