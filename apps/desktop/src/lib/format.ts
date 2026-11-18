import type { Category, FidelityLevel, Verification } from "./types";

const sizeFmt = new Intl.NumberFormat("fr-FR", { maximumFractionDigits: 1 });

export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} o`;
  const units = ["Ko", "Mo", "Go", "To"];
  let v = bytes / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${sizeFmt.format(v)} ${units[i]}`;
}

export function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms} ms`;
  const s = ms / 1000;
  if (s < 60) return `${sizeFmt.format(s)} s`;
  const m = Math.floor(s / 60);
  return `${m} min ${Math.round(s % 60)} s`;
}

export const categoryLabel: Record<Category, string> = {
  image: "Images",
  audio: "Audio",
  video: "Vidéo",
  document: "Documents",
  spreadsheet: "Tableurs",
  presentation: "Présentations",
  pdf: "PDF et mise en page",
  ebook: "Livres électroniques",
  archive: "Archives",
  data: "Données",
  subtitle: "Sous-titres",
  font: "Polices",
  model3d: "3D et CAO",
  geo: "Géographie",
  other: "Divers",
};

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

export const fidelityLabel: Record<FidelityLevel, string> = {
  lossless: "Sans perte",
  partial: "Contenu intact",
  depends: "Selon le contenu",
  lossy: "Avec perte",
};

export const fidelityRank: Record<FidelityLevel, number> = { lossless: 0, partial: 1, depends: 2, lossy: 3 };

export function worst(a: FidelityLevel, b: FidelityLevel): FidelityLevel {
  return fidelityRank[a] >= fidelityRank[b] ? a : b;
}

export function verificationText(v: Verification): string {
  switch (v.level) {
    case "decoded":
      return `Décodé entièrement : ${v.detail}`;
    case "structural":
      return `Structure contrôlée : ${v.detail}`;
    case "signature_only":
      return "Seule la signature du fichier a pu être contrôlée";
    case "failed":
      return `Échec de la vérification : ${v.detail}`;
  }
}

/** Nom de fichier d'un chemin Windows ou Unix. */
export function baseName(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}
