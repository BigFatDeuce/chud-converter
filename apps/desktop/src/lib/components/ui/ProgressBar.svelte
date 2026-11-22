<script lang="ts">
  // Barre de progression lumineuse. Sans valeur : progression indéterminée (une lueur glisse ;
  // immobile, en hachures, quand les animations sont désactivées).
  let { value = null, label, thin = false }: { value?: number | null; label: string; thin?: boolean } = $props();
  const pct = $derived(value == null ? null : Math.max(0, Math.min(1, value)) * 100);
</script>

<div
  class="bar"
  class:thin
  class:indeterminate={pct == null}
  role="progressbar"
  aria-label={label}
  aria-valuemin={pct == null ? undefined : 0}
  aria-valuemax={pct == null ? undefined : 100}
  aria-valuenow={pct == null ? undefined : Math.round(pct)}
>
  <span style:width={pct != null ? `${Math.max(2, pct)}%` : undefined}></span>
</div>

<style>
  .bar {
    position: relative;
    height: 6px;
    border-radius: 99px;
    background: var(--surface-3);
    overflow: hidden;
    width: 100%;
  }

  .thin {
    height: 4px;
  }

  span {
    display: block;
    height: 100%;
    border-radius: inherit;
    background: linear-gradient(90deg, var(--accent), var(--accent-2) 60%, var(--accent-3));
    box-shadow: 0 0 12px var(--accent);
    transition: width var(--t-slow) var(--ease);
  }

  .indeterminate span {
    position: absolute;
    width: 35%;
    animation: sweep 1.3s var(--ease) infinite;
  }

  @keyframes sweep {
    from {
      left: -35%;
    }
    to {
      left: 100%;
    }
  }

  /* Animations coupées : hachures immobiles, la barre reste lisible comme « en cours ». */
  :global([data-motion="off"]) .indeterminate span,
  :global([data-motion="reduced"]) .indeterminate span {
    width: 100%;
    left: 0;
    opacity: 0.6;
    background: repeating-linear-gradient(-45deg, var(--accent) 0 6px, var(--accent-2) 6px 12px);
    box-shadow: none;
  }
</style>
