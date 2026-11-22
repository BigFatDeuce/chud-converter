<script lang="ts">
  // Message bref en bas de la fenêtre, avec un bouton facultatif.
  import { L } from "../../i18n.svelte";
  import Icon from "../Icon.svelte";

  let { text, action = null, onclose }: { text: string; action?: { label: string; run: () => void } | null; onclose: () => void } = $props();
</script>

<div class="toast pop" role="status">
  <span class="spark"><Icon name="sparkle" size={14} /></span>
  <span class="text selectable">{text}</span>
  {#if action}
    <button class="btn small" onclick={() => action.run()}>{action.label}</button>
  {/if}
  <button class="icon-btn" aria-label={L("Fermer", "Close")} onclick={onclose}><Icon name="x" size={14} /></button>
</div>

<style>
  .toast {
    position: fixed;
    bottom: 20px;
    left: 50%;
    translate: -50% 0;
    z-index: 60;
    display: flex;
    align-items: center;
    gap: 10px;
    width: max-content;
    max-width: min(640px, calc(100vw - 32px));
    padding: 8px 8px 8px 12px;
    border-radius: 14px;
    border: 1px solid var(--border-strong);
    background: var(--surface-solid);
    box-shadow: var(--shadow-lg);
  }

  .spark {
    display: grid;
    place-items: center;
    flex: none;
    width: 24px;
    height: 24px;
    border-radius: 8px;
    color: var(--accent);
    background: var(--accent-soft);
  }

  .text {
    min-width: 0;
  }
</style>
