<script lang="ts">
  // Spec section 9: `console.*` -> Console panel. Shows the last send's
  // combined pre-request/post-response output, in call order -- an
  // "error" entry can come from an actual `console.error()` call or,
  // per spec section 16, from a post-response script that threw (the
  // response itself still succeeded, so that's surfaced here rather
  // than failing the whole send).

  import type { ConsoleEntryDto } from "./types";

  let { entries }: { entries: ConsoleEntryDto[] } = $props();
</script>

<div class="console">
  {#if entries.length === 0}
    <p class="empty">No console output yet -- runs when a pre-request or post-response script logs something.</p>
  {:else}
    {#each entries as entry, i (i)}
      <div class="line level-{entry.level}">
        <span class="tag">{entry.level}</span>
        <span class="message">{entry.message}</span>
      </div>
    {/each}
  {/if}
</div>

<style>
  .console {
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 0.6rem 0.75rem;
    font-family: ui-monospace, "SF Mono", Consolas, monospace;
    font-size: 0.8rem;
    max-height: 16rem;
    overflow: auto;
  }

  .empty {
    margin: 0;
    opacity: 0.7;
    font-family: inherit;
  }

  .line {
    display: flex;
    gap: 0.5rem;
    align-items: baseline;
    padding: 0.15rem 0;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .tag {
    flex-shrink: 0;
    text-transform: uppercase;
    font-size: 0.68rem;
    font-weight: 700;
    opacity: 0.6;
    width: 3.2rem;
  }

  .level-warn .tag,
  .level-warn .message {
    color: #d29922;
  }

  .level-error .tag,
  .level-error .message {
    color: var(--danger);
  }
</style>
