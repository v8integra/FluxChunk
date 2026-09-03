<script lang="ts">
  import Self from "./DiffTree.svelte";
  import type { DiffNode } from "./types";

  let { node, isLast = true }: { node: DiffNode; isLast?: boolean } = $props();

  let manuallyToggled = $state<boolean | null>(null);
  let open = $derived(manuallyToggled ?? true);

  function toggle() {
    manuallyToggled = !open;
  }

  function formatScalar(v: unknown): string {
    if (v === null || v === undefined) return "null";
    if (typeof v === "string") return `"${v}"`;
    return String(v);
  }

  function marker(status: DiffNode["status"]): string {
    switch (status) {
      case "added":
        return "+";
      case "removed":
        return "-";
      case "changed":
        return "~";
      default:
        return "";
    }
  }
</script>

{#if node.children.length > 0}
  <div class="diff-node status-{node.status}">
    <button type="button" class="toggle" onclick={toggle} aria-label={open ? "Collapse" : "Expand"}>{open ? "▾" : "▸"}</button>
    <span class="marker">{marker(node.status)}</span>
    {#if node.key !== null}<span class="diff-key">{node.key}</span><span class="colon">:</span>{/if}
  </div>
  {#if open}
    <div class="diff-children">
      {#each node.children as child, i (child.key ?? i)}
        <Self node={child} isLast={i === node.children.length - 1} />
      {/each}
    </div>
  {/if}
{:else}
  <div class="diff-node leaf status-{node.status}">
    <span class="toggle-spacer"></span>
    <span class="marker">{marker(node.status)}</span>
    {#if node.key !== null}<span class="diff-key">{node.key}</span><span class="colon">:</span>{/if}
    {#if node.status === "changed"}
      <span class="old-value">{formatScalar(node.old_value)}</span>
      <span class="arrow">&rarr;</span>
      <span class="new-value">{formatScalar(node.new_value)}</span>
    {:else if node.status === "added"}
      <span class="new-value">{formatScalar(node.new_value)}</span>
    {:else if node.status === "removed"}
      <span class="old-value">{formatScalar(node.old_value)}</span>
    {:else}
      <span class="unchanged-value">{formatScalar(node.old_value)}</span>
    {/if}
  </div>
{/if}

<style>
  .diff-node {
    display: flex;
    align-items: baseline;
    gap: 0.3rem;
    font-family: "SFMono-Regular", Consolas, "Liberation Mono", Menlo, monospace;
    font-size: 0.85rem;
    line-height: 1.6;
    white-space: pre;
    padding: 0 0.3rem;
    border-radius: 3px;
  }

  .diff-children {
    padding-left: 1.1rem;
    border-left: 1px solid rgba(127, 127, 127, 0.2);
    margin-left: 0.35rem;
  }

  .toggle {
    background: none;
    border: none;
    padding: 0;
    width: 0.9rem;
    flex-shrink: 0;
    cursor: pointer;
    font-size: 0.7rem;
    opacity: 0.7;
  }

  .toggle-spacer {
    width: 0.9rem;
    flex-shrink: 0;
  }

  .marker {
    width: 0.8rem;
    display: inline-block;
    font-weight: 700;
  }

  .diff-key {
    opacity: 0.8;
  }

  .arrow {
    opacity: 0.6;
  }

  .status-added {
    background: rgba(46, 160, 67, 0.15);
  }
  .status-added .marker {
    color: #2ea043;
  }

  .status-removed {
    background: rgba(248, 81, 73, 0.15);
  }
  .status-removed .marker {
    color: #f85149;
  }

  .status-changed {
    background: rgba(210, 153, 34, 0.15);
  }
  .status-changed .marker {
    color: #d29922;
  }

  .status-unchanged {
    opacity: 0.65;
  }

  .old-value {
    color: #f85149;
    text-decoration: line-through;
  }

  .new-value {
    color: #2ea043;
  }
</style>
