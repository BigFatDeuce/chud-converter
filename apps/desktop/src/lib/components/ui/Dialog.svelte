<script lang="ts">
  // Fenêtre modale : fond voilé, carte centrée, Échap pour fermer, focus gardé à l'intérieur.
  import type { Snippet } from "svelte";

  let {
    labelledby,
    onclose,
    busy = false,
    width = 580,
    machine,
    children,
    el = $bindable(),
  }: {
    labelledby: string;
    onclose: () => void;
    busy?: boolean;
    width?: number;
    machine?: string;
    children: Snippet;
    el?: HTMLDivElement;
  } = $props();

  function onkey(ev: KeyboardEvent) {
    if (ev.key === "Escape") {
      onclose();
      return;
    }
    if (ev.key !== "Tab" || !el) return;
    const items = [...el.querySelectorAll<HTMLElement>("button:not(:disabled), input:not(:disabled), select, [href], [tabindex]:not([tabindex='-1'])")];
    if (!items.length) return;
    const first = items[0];
    const last = items[items.length - 1];
    if (ev.shiftKey && document.activeElement === first) {
      ev.preventDefault();
      last.focus();
    } else if (!ev.shiftKey && document.activeElement === last) {
      ev.preventDefault();
      first.focus();
    }
  }
</script>

<svelte:window onkeydown={onkey} />

<div class="backdrop">
  <div
    class="dialog panel pop"
    style:--w="{width}px"
    role="dialog"
    aria-modal="true"
    aria-labelledby={labelledby}
    aria-busy={busy}
    data-machine={machine}
    bind:this={el}
  >
    {@render children()}
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: grid;
    place-items: center;
    padding: 20px;
    background: var(--backdrop);
    animation: fade var(--t) var(--ease) both;
  }

  .dialog {
    position: relative;
    width: min(var(--w), 100%);
    max-height: calc(100vh - 40px);
    overflow-y: auto;
    padding: 26px 26px 22px;
    display: grid;
    gap: 14px;
    background: var(--surface-solid);
    box-shadow: var(--shadow-lg);
  }

  /* Liseré lumineux en haut de la carte. */
  .dialog::before {
    content: "";
    position: absolute;
    inset: 0 20% auto;
    height: 1px;
    background: linear-gradient(90deg, transparent, var(--accent-3), var(--accent-2), transparent);
    opacity: 0.8;
  }
</style>
