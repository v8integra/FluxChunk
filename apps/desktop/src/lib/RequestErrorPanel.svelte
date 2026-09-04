<script lang="ts" module>
  export type RequestFailureKind = "dns" | "timeout" | "tls" | "connection_refused" | "other" | "internal";
</script>

<script lang="ts">
  // Spec section 16: "Failed requests: inline explanation in the
  // response panel (DNS failure, timeout, TLS error, etc. --
  // categorized, not a generic error badge)". Sits where the response
  // would otherwise go, same as a normal response filling that column.

  const LABELS: Record<string, { title: string }> = {
    dns: { title: "DNS lookup failed" },
    timeout: { title: "Request timed out" },
    tls: { title: "TLS/certificate error" },
    connection_refused: { title: "Connection refused" },
    other: { title: "Request failed" },
    internal: { title: "Couldn't send the request" },
  };

  let { kind, message }: { kind: string; message: string } = $props();
  let label = $derived(LABELS[kind] ?? LABELS.other);
</script>

<div class="panel">
  <h3>{label.title}</h3>
  <p>{message}</p>
</div>

<style>
  .panel {
    background: var(--bg-elevated);
    border: 1px solid var(--danger);
    border-radius: var(--radius);
    padding: 1rem;
  }

  h3 {
    margin: 0 0 0.4rem;
    color: var(--danger);
    font-size: 0.95rem;
  }

  p {
    margin: 0;
    font-size: 0.88rem;
  }
</style>
