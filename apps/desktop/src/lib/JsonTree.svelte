<script lang="ts">
  import Self from "./JsonTree.svelte";

  // NOTE on scale: this renders one Svelte component per JSON node, not a
  // windowed/virtualized list -- fine for typical API responses, but a
  // JSON body with tens of thousands of nodes (still possible under the
  // 5MB size gate in response.rs) will be slow to render. True DOM
  // virtualization is a deliberate follow-up, not done here -- see
  // crates/engine/src/response.rs's module docs.

  let {
    value,
    path = [],
    label = null,
    isLast = true,
    matchSet,
    expandSet,
    currentMatchKey,
  }: {
    value: unknown;
    path?: (string | number)[];
    label?: string | number | null;
    isLast?: boolean;
    matchSet: Set<string>;
    expandSet: Set<string>;
    currentMatchKey: string | null;
  } = $props();

  let key = $derived(JSON.stringify(path));
  let isMatch = $derived(matchSet.has(key));
  let isCurrent = $derived(currentMatchKey === key);

  let manuallyToggled = $state<boolean | null>(null);
  let open = $derived(manuallyToggled ?? (path.length === 0 || expandSet.has(key)));

  function toggle() {
    manuallyToggled = !open;
  }

  function isContainer(v: unknown): v is unknown[] | Record<string, unknown> {
    return v !== null && typeof v === "object";
  }

  function entries(v: unknown[] | Record<string, unknown>): [string | number, unknown][] {
    return Array.isArray(v) ? v.map((item, i) => [i, item]) : Object.entries(v);
  }

  function typeClass(v: unknown): string {
    if (v === null) return "json-null";
    switch (typeof v) {
      case "string":
        return "json-string";
      case "number":
        return "json-number";
      case "boolean":
        return "json-bool";
      default:
        return "";
    }
  }

  function formatLeaf(v: unknown): string {
    if (v === null) return "null";
    if (typeof v === "string") return `"${v}"`;
    return String(v);
  }
</script>

{#if isContainer(value)}
  {@const items = entries(value)}
  {@const isArray = Array.isArray(value)}
  <div class="node" data-json-path={key}>
    <button type="button" class="toggle" onclick={toggle} aria-label={open ? "Collapse" : "Expand"}>{open ? "▾" : "▸"}</button>
    {#if label !== null}<span class="json-key">{typeof label === "number" ? label : `"${label}"`}</span><span class="colon">:</span>{/if}
    <span class="bracket">{isArray ? "[" : "{"}</span>
    {#if !open}
      <span class="summary">{items.length} {isArray ? "item" : "key"}{items.length === 1 ? "" : "s"}</span>
      <span class="bracket">{isArray ? "]" : "}"}</span>{#if !isLast}<span class="comma">,</span>{/if}
    {/if}
  </div>
  {#if open}
    <div class="children">
      {#each items as [k, v], i (k)}
        <Self value={v} path={[...path, k]} label={k} isLast={i === items.length - 1} {matchSet} {expandSet} {currentMatchKey} />
      {/each}
    </div>
    <div class="bracket close">{isArray ? "]" : "}"}{#if !isLast}<span class="comma">,</span>{/if}</div>
  {/if}
{:else}
  <div class="node leaf" class:match={isMatch} class:current={isCurrent} data-json-path={key}>
    <span class="toggle-spacer"></span>
    {#if label !== null}<span class="json-key">{typeof label === "number" ? label : `"${label}"`}</span><span class="colon">:</span>{/if}
    <span class={typeClass(value)}>{formatLeaf(value)}</span>{#if !isLast}<span class="comma">,</span>{/if}
  </div>
{/if}

<style>
  .node {
    display: flex;
    align-items: baseline;
    gap: 0.3rem;
    font-family: "SFMono-Regular", Consolas, "Liberation Mono", Menlo, monospace;
    font-size: 0.85rem;
    line-height: 1.5;
    white-space: pre;
  }

  .children {
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

  .json-key {
    color: #a626a4;
  }

  .json-string {
    color: #50a14f;
  }

  .json-number {
    color: #4078f2;
  }

  .json-bool {
    color: #c18401;
  }

  .json-null {
    color: #888;
  }

  .summary {
    opacity: 0.55;
    font-style: italic;
  }

  .comma {
    opacity: 0.6;
  }

  .leaf.match {
    background: rgba(255, 220, 0, 0.18);
    border-radius: 3px;
  }

  .leaf.current {
    background: rgba(255, 160, 0, 0.45);
    outline: 1px solid rgba(255, 160, 0, 0.8);
    border-radius: 3px;
  }

  @media (prefers-color-scheme: dark) {
    .json-key {
      color: #c678dd;
    }
    .json-string {
      color: #98c379;
    }
    .json-number {
      color: #61afef;
    }
    .json-bool {
      color: #e5c07b;
    }
    .json-null {
      color: #999;
    }
  }
</style>
