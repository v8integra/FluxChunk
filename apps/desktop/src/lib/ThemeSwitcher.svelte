<script lang="ts" module>
  export const THEMES = ["light", "dark", "blue", "green", "red", "pink", "silver"] as const;
  export type Theme = (typeof THEMES)[number];
</script>

<script lang="ts">
  // "7 options as vertical DIP switches... radio-button behavior
  // visualized as physical toggles -- selecting one flips it on and all
  // others off" (spec section 10). There's exactly one `theme` value, so
  // radio behavior falls out naturally from onChange always replacing it.

  let { theme, onChange }: { theme: Theme; onChange: (t: Theme) => void } = $props();

  function label(t: Theme): string {
    return t[0].toUpperCase() + t.slice(1);
  }
</script>

<div class="dip-switches" role="radiogroup" aria-label="Theme">
  {#each THEMES as t (t)}
    <button type="button" class="dip" class:on={theme === t} role="radio" aria-checked={theme === t} onclick={() => onChange(t)}>
      <span class="track"><span class="thumb"></span></span>
      <span class="dip-label">{label(t)}</span>
    </button>
  {/each}
</div>

<style>
  /* The whole board rotated 90deg from the first pass: switches run in a
     horizontal row, and each individual toggle is itself vertical (track
     stood on end, thumb slides up/down) -- closer to how a real DIP
     switch pack reads left-to-right with each slider flipping up for on. */

  .dip-switches {
    display: flex;
    flex-direction: row;
    align-items: flex-end;
    gap: 0.5rem;
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 0.4rem 0.5rem;
  }

  .dip {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.3rem;
    background: transparent;
    border: none;
    padding: 0;
    cursor: pointer;
    font-size: 0.65rem;
    color: var(--text);
    border-radius: 4px;
  }

  .dip:hover {
    background: var(--bg-hover);
  }

  .track {
    width: 0.9rem;
    height: 1.6rem;
    border-radius: 999px;
    background: var(--border);
    position: relative;
    flex-shrink: 0;
    transition: background 0.15s ease;
  }

  .thumb {
    position: absolute;
    left: 1px;
    bottom: 1px;
    width: 0.75rem;
    height: 0.75rem;
    border-radius: 50%;
    background: var(--bg-elevated);
    transition: transform 0.15s ease;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.3);
  }

  .dip.on .track {
    background: var(--accent);
  }

  .dip.on .thumb {
    /* Off rests at the bottom; on flips it up to the top. */
    transform: translateY(-0.72rem);
  }

  .dip-label {
    white-space: nowrap;
  }
</style>
