<script lang="ts" module>
  export type TourStep = { title: string; body: string };
</script>

<script lang="ts">
  // Spec section 12: "~5 steps... Skip available at every step, progress
  // shown as dots, Back/Next navigation." Deliberately a small floating
  // card, not a full-screen modal -- the whole point of "live, functional
  // demo, not static screenshots" is that the user can actually click the
  // real Send button (etc.) while this is open, which a blocking overlay
  // would prevent. `steps` is a plain prop specifically so this same
  // component can be reused for the "What's New" re-launch (spec section
  // 12) with different content once that exists.

  let {
    steps,
    step,
    onNext,
    onBack,
    onSkip,
    onFinish,
  }: {
    steps: TourStep[];
    step: number;
    onNext: () => void;
    onBack: () => void;
    onSkip: () => void;
    onFinish: () => void;
  } = $props();

  let current = $derived(steps[step]);
  let isLast = $derived(step === steps.length - 1);
</script>

<div class="tour-card" role="dialog" aria-label="Tour">
  <div class="tour-header">
    <strong>{current.title}</strong>
    <button type="button" class="skip" onclick={onSkip}>Skip</button>
  </div>
  <p class="body">{current.body}</p>
  <div class="tour-footer">
    <div class="dots" aria-hidden="true">
      {#each steps as _, i (i)}
        <span class="dot" class:active={i === step}></span>
      {/each}
    </div>
    <div class="nav">
      <button type="button" onclick={onBack} disabled={step === 0}>Back</button>
      <button type="button" class="primary" onclick={isLast ? onFinish : onNext}>{isLast ? "Done" : "Next"}</button>
    </div>
  </div>
</div>

<style>
  .tour-card {
    position: fixed;
    right: 1.5rem;
    bottom: 1.5rem;
    width: 20rem;
    background: var(--bg-elevated);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: 0 6px 20px rgba(0, 0, 0, 0.3);
    padding: 1rem;
    z-index: 90;
  }

  .tour-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
  }

  .skip {
    background: transparent;
    border: none;
    padding: 0.15rem 0.3rem;
    font-size: 0.78rem;
    color: var(--text-muted);
  }

  .body {
    font-size: 0.85rem;
    line-height: 1.4;
    margin: 0.5rem 0 0.9rem;
  }

  .tour-footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .dots {
    display: flex;
    gap: 0.3rem;
  }

  .dot {
    width: 0.4rem;
    height: 0.4rem;
    border-radius: 50%;
    background: var(--border);
  }

  .dot.active {
    background: var(--accent);
  }

  .nav {
    display: flex;
    gap: 0.4rem;
  }

  .nav button {
    font-size: 0.8rem;
    padding: 0.25rem 0.6rem;
  }

  .primary {
    background: var(--accent);
    color: var(--accent-text);
    border-color: var(--accent);
  }
</style>
