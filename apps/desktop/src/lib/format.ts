import type { Category, FidelityLevel, Verification } from "./types";
import { L, i18n, locale } from "./i18n.svelte";

const fmtCache = new Map<string, Intl.NumberFormat>();
function num(v: number): string {
  const loc = locale();
  let f = fmtCache.get(loc);
  if (!f) {
    f = new Intl.NumberFormat(loc, { maximumFractionDigits: 1 });
    fmtCache.set(loc, f);
  }
  return f.format(v);
}

export function formatSize(bytes: number): string {
  const en = i18n.lang === "en";
  if (bytes < 1024) return `${bytes} ${en ? "B" : "o"}`;
  const units = en ? ["KB", "MB", "GB", "TB"] : ["Ko", "Mo", "Go", "To"];
  let v = bytes / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${num(v)} ${units[i]}`;
}

export function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms} ms`;
  const s = ms / 1000;
  if (s < 60) return `${num(s)} s`;
  const m = Math.floor(s / 60);
  return `${m} min ${Math.round(s % 60)} s`;
}

const CATEGORY: Record<Category, [string, string]> = {
  image: ["Images", "Images"],
  audio: ["Audio", "Audio"],
  video: ["Vidéo", "Video"],
  document: ["Documents", "Documents"],
  spreadsheet: ["Tableurs", "Spreadsheets"],
  presentation: ["Présentations", "Presentations"],
  pdf: ["PDF et mise en page", "PDF and page layout"],
  ebook: ["Livres électroniques", "E-books"],
  archive: ["Archives", "Archives"],
  data: ["Données", "Data"],
  subtitle: ["Sous-titres", "Subtitles"],
  font: ["Polices", "Fonts"],
  model3d: ["3D et CAO", "3D and CAD"],
  geo: ["Géographie", "Geography"],
  other: ["Divers", "Other"],
};

export function categoryLabel(c: Category): string {
  return L(...CATEGORY[c]);
}

export const categoryOrder: Category[] = [
  "video",
  "audio",
  "image",
  "subtitle",
  "archive",
  "data",
  "document",
  "spreadsheet",
  "presentation",
  "pdf",
  "ebook",
  "font",
  "model3d",
  "geo",
  "other",
];

const FIDELITY: Record<FidelityLevel, [string, string]> = {
  lossless: ["Sans perte", "Lossless"],
  partial: ["Contenu intact", "Content intact"],
  depends: ["Selon le contenu", "Depends on content"],
  lossy: ["Avec perte", "Lossy"],
};

export function fidelityLabel(f: FidelityLevel): string {
  return L(...FIDELITY[f]);
}

export const fidelityRank: Record<FidelityLevel, number> = { lossless: 0, partial: 1, depends: 2, lossy: 3 };

export function worst(a: FidelityLevel, b: FidelityLevel): FidelityLevel {
  return fidelityRank[a] >= fidelityRank[b] ? a : b;
}

export function verificationText(v: Verification): string {
  switch (v.level) {
    case "decoded":
      return L(`Décodé entièrement : ${v.detail}`, `Fully decoded: ${v.detail}`);
    case "structural":
      return L(`Structure contrôlée : ${v.detail}`, `Structure checked: ${v.detail}`);
    case "signature_only":
      return L("Seule la signature du fichier a pu être contrôlée", "Only the file signature could be checked");
    case "failed":
      return L(`Échec de la vérification : ${v.detail}`, `Verification failed: ${v.detail}`);
  }
}

/** Nom de fichier d'un chemin Windows ou Unix. */
export function baseName(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}
