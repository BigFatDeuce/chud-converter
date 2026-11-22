<script lang="ts">
  // Pastille d'état : un point coloré et un mot (Prêt, En cours, Échec…).
  import type { Snippet } from "svelte";

  type Tone = "ok" | "partial" | "warn" | "danger" | "accent" | "neutral";
  let { tone = "neutral", pulse = false, title, children }: { tone?: Tone; pulse?: boolean; title?: string; children: Snippet } = $props();
</script>

<span class="pill {tone}" {title}>
  <span class="dot" class:pulse></span>
  {@render children()}
</span>

<style>
  .pill {
    --c: var(--faint);
    --s: var(--surface-3);
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 23px;
    padding: 0 10px 0 8px;
    border-radius: 999px;
    background: var(--s);
    color: var(--c);
    font-size: 12px;
    font-weight: 600;
    white-space: nowrap;
  }

  .neutral {
    --c: var(--muted);
  }

  .ok {
    --c: var(--ok);
    --s: var(--ok-soft);
  }

  .partial {
    --c: var(--partial);
    --s: var(--partial-soft);
  }

  .warn {
    --c: var(--depends);
    --s: var(--depends-soft);
  }

  .danger {
    --c: var(--lossy);
    --s: var(--lossy-soft);
  }

  .accent {
    --c: var(--accent);
    --s: var(--accent-soft);
  }

  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: currentColor;
    box-shadow: 0 0 8px currentColor;
  }

  .pulse {
    animation: pulse 1.6s ease-in-out infinite;
  }
</style>
