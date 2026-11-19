// Textes courts sur l'état d'un moteur pendant une installation, partagés par l'accueil et le
// bandeau des moteurs manquants.

import { L } from "./i18n.svelte";
import { formatSize } from "./format";
import type { EngineOp } from "./machines";
import type { EngineView, InstallOffer } from "./types";

/** Taille du bouton « Installer (…) » : octets à télécharger, ou le gestionnaire du système. */
export function offerSize(o: InstallOffer): string {
  return o.system ? L(`via ${o.system.manager}`, `via ${o.system.manager}`) : formatSize(o.size);
}

/** Ce que l'installation fera hors du dossier de Chud Converter (à dire avant de confirmer). */
export function offerSystemNotes(o: InstallOffer): string[] {
  const out: string[] = [];
  if (o.system) {
    out.push(
      L(
        `Installé par le gestionnaire de paquets de votre système (${o.system.manager}) : ${o.system.packages.join(", ")}. Votre mot de passe administrateur sera demandé dans une fenêtre du système.`,
        `Installed by your system's package manager (${o.system.manager}): ${o.system.packages.join(", ")}. Your administrator password will be asked in a system window.`,
      ),
      L(
        "Le paquet appartient ensuite à votre distribution : il se met à jour et se retire avec elle, pas depuis Chud Converter.",
        "The package then belongs to your distribution: it is updated and removed with it, not from Chud Converter.",
      ),
    );
  }
  if (o.runtime) {
    out.push(
      L(
        "Il a besoin du runtime Visual C++ de Microsoft, absent de cet ordinateur : il sera installé aussi, depuis Microsoft (empreinte vérifiée). Windows vous demandera une autorisation.",
        "It needs the Microsoft Visual C++ runtime, missing on this computer: it will be installed too, from Microsoft (fingerprint checked). Windows will ask for your permission.",
      ),
    );
  }
  return out;
}

/** « téléchargement 45 % », « prêt », « en attente »… */
export function engineStatus(e: EngineView, op: EngineOp | null | undefined): string {
  if (op?.state === "running") {
    const p = op.progress;
    if (op.cancelling) return L("arrêt…", "stopping…");
    if (!p) return L("préparation…", "preparing…");
    if (p.phase === "downloading") {
      const n = Math.round((p.received * 100) / Math.max(1, p.total));
      return L(`téléchargement ${n} %`, `downloading ${n}%`);
    }
    if (p.phase === "verifying") return L("vérification SHA-256…", "checking SHA-256…");
    if (p.phase === "runtime") return L("runtime Visual C++…", "Visual C++ runtime…");
    if (p.phase === "system") return p.line;
    if (p.phase === "extracting") {
      if (!p.total) return L("extraction…", "extracting…");
      const n = Math.round((p.done * 100) / p.total);
      return L(`extraction ${n} %`, `extracting ${n}%`);
    }
    return L("essai…", "trying…");
  }
  if (op?.state === "failed") return L("échec", "failed");
  if (e.state === "ready") return L("prêt", "ready");
  return L("en attente", "waiting");
}

/** Avancement entre 0 et 1, ou null quand il n'est pas mesurable (barre indéterminée). */
export function engineFraction(op: EngineOp | null | undefined): number | null {
  if (op?.state !== "running") return null;
  const p = op.progress;
  if (p?.phase === "downloading") return p.received / Math.max(1, p.total);
  if (p?.phase === "extracting" && p.total) return p.done / p.total;
  return null;
}
