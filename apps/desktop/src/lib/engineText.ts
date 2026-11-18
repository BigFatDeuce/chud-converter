// Textes courts sur l'état d'un moteur pendant une installation, partagés par l'accueil et le
// bandeau des moteurs manquants.

import { L } from "./i18n.svelte";
import type { EngineOp } from "./machines";
import type { EngineView } from "./types";

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
