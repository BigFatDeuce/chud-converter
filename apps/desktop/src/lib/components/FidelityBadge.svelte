<script lang="ts">
  import { fidelityLabel } from "../format";
  import type { FidelityLevel } from "../types";

  let { level, title }: { level: FidelityLevel; title?: string } = $props();
</script>

<span class="badge {level}" {title}>
  <span class="dot"></span>
  {fidelityLabel[level]}
</span>

<style>
  .badge {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 22px;
    padding: 0 8px;
    border-radius: 999px;
    font-size: 12px;
    font-weight: 600;
    white-space: nowrap;
  }

  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: currentColor;
  }

  .lossless {
    color: var(--ok);
    background: var(--ok-soft);
  }

  .partial {
    color: var(--partial);
    background: var(--partial-soft);
  }

  .depends {
    color: var(--depends);
    background: var(--depends-soft);
  }

  .lossy {
    color: var(--lossy);
    background: var(--lossy-soft);
  }
</style>
