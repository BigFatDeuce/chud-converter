// Apparence de l'interface : thème (système, sombre, clair) et animations (automatiques,
// réduites, désactivées). Préférences de cet ordinateur, gardées dans le stockage local de la
// fenêtre ; les autres fenêtres (conversion rapide) suivent les changements en direct.
// Le choix est appliqué en attributs sur <html> : data-theme et data-motion (voir app.css).

import { motionLevel, type Motion } from "./machines";

export type Theme = "system" | "dark" | "light";
export type { Motion };

const KEY = "chud.ui";
const THEMES: Theme[] = ["system", "dark", "light"];
const MOTIONS: Motion[] = ["auto", "reduced", "off"];

interface Saved {
  theme: Theme;
  motion: Motion;
}

function read(): Saved {
  const out: Saved = { theme: "system", motion: "auto" };
  try {
    const raw = localStorage.getItem(KEY);
    if (raw) {
      const v = JSON.parse(raw) as Partial<Saved>;
      if (v.theme && THEMES.includes(v.theme)) out.theme = v.theme;
      if (v.motion && MOTIONS.includes(v.motion)) out.motion = v.motion;
    }
  } catch {
    // Stockage indisponible : valeurs par défaut.
  }
  return out;
}

function media(q: string): MediaQueryList | null {
  return typeof matchMedia === "function" ? matchMedia(q) : null;
}

class Ui {
  theme = $state<Theme>("system");
  motion = $state<Motion>("auto");
  systemReduced = $state(false);
  systemDark = $state(true);

  level = $derived(motionLevel(this.motion, this.systemReduced));
  dark = $derived(this.theme === "dark" || (this.theme === "system" && this.systemDark));

  constructor() {
    const s = read();
    this.theme = s.theme;
    this.motion = s.motion;
    const rm = media("(prefers-reduced-motion: reduce)");
    const dk = media("(prefers-color-scheme: dark)");
    this.systemReduced = !!rm?.matches;
    this.systemDark = dk ? dk.matches : true;
    rm?.addEventListener("change", (e) => {
      this.systemReduced = e.matches;
      this.#apply();
    });
    dk?.addEventListener("change", (e) => (this.systemDark = e.matches));
    // Changement fait dans une autre fenêtre de l'application.
    if (typeof window !== "undefined") {
      window.addEventListener("storage", (e) => {
        if (e.key !== KEY) return;
        const n = read();
        this.theme = n.theme;
        this.motion = n.motion;
        this.#apply();
      });
    }
    this.#apply();
  }

  #apply() {
    if (typeof document === "undefined") return;
    const root = document.documentElement;
    root.dataset.theme = this.theme;
    root.dataset.motion = motionLevel(this.motion, this.systemReduced);
  }

  #save() {
    try {
      localStorage.setItem(KEY, JSON.stringify({ theme: this.theme, motion: this.motion }));
    } catch {
      // Préférence non mémorisée : sans gravité.
    }
  }

  setTheme(t: Theme) {
    this.theme = t;
    this.#apply();
    this.#save();
  }

  setMotion(m: Motion) {
    this.motion = m;
    this.#apply();
    this.#save();
  }
}

export const ui = new Ui();
