<script lang="ts" module>
  import type { Category } from "../../types";
  import type { IconName } from "../Icon.svelte";

  // Une icône et une teinte par famille de fichiers : la liste se lit d'un coup d'œil.
  const GLYPH: Record<Category, { icon: IconName; hue: number }> = {
    image: { icon: "image", hue: 190 },
    video: { icon: "video", hue: 265 },
    audio: { icon: "audio", hue: 300 },
    document: { icon: "document", hue: 228 },
    spreadsheet: { icon: "spreadsheet", hue: 155 },
    presentation: { icon: "presentation", hue: 28 },
    pdf: { icon: "pdf", hue: 350 },
    ebook: { icon: "ebook", hue: 40 },
    archive: { icon: "archive", hue: 210 },
    data: { icon: "data", hue: 175 },
    subtitle: { icon: "subtitle", hue: 245 },
    font: { icon: "font", hue: 320 },
    model3d: { icon: "cube", hue: 200 },
    geo: { icon: "pin", hue: 140 },
    other: { icon: "file", hue: 230 },
  };

  export function glyphFor(category: Category | null | undefined, dir = false): { icon: IconName; hue: number } {
    if (dir) return { icon: "folder", hue: 45 };
    return category ? GLYPH[category] : { icon: "file", hue: 230 };
  }
</script>

<script lang="ts">
  import Icon from "../Icon.svelte";

  let { category, dir = false, size = 34, broken = false }: { category?: Category | null; dir?: boolean; size?: number; broken?: boolean } = $props();
  const g = $derived(glyphFor(category, dir));
</script>

<span class="glyph" class:broken style:--h={g.hue} style:--s="{size}px">
  <Icon name={broken ? "alert" : g.icon} size={Math.round(size * 0.5)} stroke={1.7} />
</span>

<style>
  .glyph {
    display: grid;
    place-items: center;
    flex: none;
    width: var(--s);
    height: var(--s);
    border-radius: calc(var(--s) * 0.3);
    color: hsl(var(--h) var(--glyph));
    background: linear-gradient(145deg, hsl(var(--h) 80% 60% / 20%), hsl(var(--h) 80% 50% / 6%));
    border: 1px solid hsl(var(--h) 80% 70% / 18%);
  }

  .broken {
    color: var(--lossy) !important;
    background: var(--lossy-soft);
    border-color: transparent;
  }
</style>
