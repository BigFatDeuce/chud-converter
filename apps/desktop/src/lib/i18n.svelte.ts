// Langue de l'interface. Les textes sont écrits côte à côte dans les composants : L("Convertir", "Convert").
// La langue vient des réglages de l'application (côté Rust) ; un changement est transmis à toutes
// les fenêtres, et l'interface est reconstruite (bloc {#key} dans App.svelte).

export type Lang = "fr" | "en";
export type LangChoice = "system" | "fr" | "en";

export const i18n = $state({ lang: "fr" as Lang });

export function L(fr: string, en: string): string {
  return i18n.lang === "en" ? en : fr;
}

/** Locale pour les nombres et les dates. */
export function locale(): string {
  return i18n.lang === "en" ? "en-US" : "fr-FR";
}
