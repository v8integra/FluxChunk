<script lang="ts">
  import Self from "./CollectionTree.svelte";

  export type TreeNode = { kind: "folder"; name: string; items: TreeNode[] } | { kind: "request"; name: string; path: string };

  let { items, onSelect, activePath }: { items: TreeNode[]; onSelect: (path: string) => void; activePath: string | null } = $props();
</script>

<ul>
  {#each items as item (item.kind === "folder" ? "f:" + item.name : item.path)}
    <li>
      {#if item.kind === "folder"}
        <details open>
          <summary>{item.name}</summary>
          <Self items={item.items} {onSelect} {activePath} />
        </details>
      {:else}
        <button type="button" class="request-item" class:active={item.path === activePath} onclick={() => onSelect(item.path)}>
          {item.name}
        </button>
      {/if}
    </li>
  {/each}
</ul>

<style>
  ul {
    list-style: none;
    margin: 0;
    padding-left: 0.9rem;
  }

  li {
    margin: 0.1rem 0;
  }

  summary {
    cursor: pointer;
    font-size: 0.85rem;
    padding: 0.15rem 0;
  }

  .request-item {
    display: block;
    width: 100%;
    text-align: left;
    background: none;
    border: none;
    padding: 0.2rem 0.4rem;
    font-size: 0.85rem;
    border-radius: 4px;
    cursor: pointer;
    color: var(--text);
  }

  .request-item:hover {
    background: var(--bg-hover);
  }

  .request-item.active {
    background: var(--bg-hover);
    box-shadow: inset 2px 0 0 var(--accent);
    font-weight: 600;
  }
</style>
