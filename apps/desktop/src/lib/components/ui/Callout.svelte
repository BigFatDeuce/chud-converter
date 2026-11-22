<script lang="ts">
  // Encadré de message : information, réussite, avertissement, erreur ou confirmation.
  // Sert aux confirmations en place (retrait, rétablissement…), aux erreurs et aux propositions.
  import type { Snippet } from "svelte";
  import Icon, { type IconName } from "../Icon.svelte";

  type Tone = "info" | "ok" | "warn" | "danger" | "accent";
  let {
    tone = "info",
    icon,
    title,
    role,
    label,
    compact = false,
    children,
    actions,
  }: {
    tone?: Tone;
    icon?: IconName | null;
    title?: string;
    role?: "alert" | "status" | "group";
    label?: string;
    compact?: boolean;
    children?: Snippet;
    actions?: Snippet;
  } = $props();

  const defaults: Record<Tone, IconName> = { info: "info", ok: "check", warn: "alert", danger: "alert", accent: "sparkle" };
  const glyph = $derived(icon === null ? null : (icon ?? defaults[tone]));
</script>

<div class="callout {tone} pop" class:compact {role} aria-label={label}>
  {#if glyph}<span class="ico"><Icon name={glyph} size={compact ? 14 : 16} /></span>{/if}
  <div class="body">
    {#if title}<p class="title">{title}</p>{/if}
    {#if children}{@render children()}{/if}
    {#if actions}<div class="actions">{@render actions()}</div>{/if}
  </div>
</div>

<style>
  .callout {
    --c: var(--partial);
    --s: var(--partial-soft);
    display: flex;
    gap: 11px;
    padding: 12px 14px;
    border-radius: var(--radius);
    border: 1px solid color-mix(in srgb, var(--c) 30%, transparent);
    background: linear-gradient(135deg, var(--s), transparent 140%), var(--surface);
    min-width: 0;
  }

  .compact {
    padding: 9px 11px;
    gap: 8px;
    font-size: 13px;
  }

  .ok {
    --c: var(--ok);
    --s: var(--ok-soft);
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

  .ico {
    flex: none;
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    margin-top: -2px;
    border-radius: 8px;
    color: var(--c);
    background: var(--s);
  }

  .compact .ico {
    width: 22px;
    height: 22px;
    margin-top: -1px;
  }

  .body {
    flex: 1;
    min-width: 0;
    display: grid;
    gap: 6px;
    align-content: start;
  }

  .title {
    font-weight: 600;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin-top: 2px;
  }
</style>
