// Machines à états de l'interface, écrites une seule fois et sans dépendance (testées par
// `npm test`). Chaque processus long a UNE valeur qui dit où il en est, au lieu de plusieurs
// drapeaux (« chargement », « erreur », « occupé »…) qui pouvaient se contredire.
//
// Les composants affichent l'état et l'exposent en `data-state` (lisible par les tests, les
// outils d'accessibilité et les agents) ; seules les fonctions ci-dessous le font changer.
// Voir docs/ARCHITECTURE.md, « Machines à états de l'interface ».

import type { EngineView, FileTargets, InstallProgress, UpdateProgress } from "./types";

// ------------------------------------------------------------------ chargement générique

/** Donnée chargée depuis le cœur. Pendant un rechargement ou après un échec, la dernière
 *  valeur connue reste affichée (`previous`). */
export type Load<T> =
  | { state: "idle" }
  | { state: "loading"; previous: T | null }
  | { state: "ready"; value: T }
  | { state: "failed"; error: string; previous: T | null };

export const idle = <T>(): Load<T> => ({ state: "idle" });

export function startLoad<T>(l: Load<T>): Load<T> {
  return { state: "loading", previous: current(l) };
}

export function finishLoad<T>(value: T): Load<T> {
  return { state: "ready", value };
}

export function failLoad<T>(l: Load<T>, error: string): Load<T> {
  return { state: "failed", error, previous: current(l) };
}

/** Valeur à afficher : la valeur chargée, ou la dernière connue. */
export function current<T>(l: Load<T>): T | null {
  switch (l.state) {
    case "idle":
      return null;
    case "ready":
      return l.value;
    default:
      return l.previous;
  }
}

// ------------------------------------------------------------------ analyse d'un fichier

/** Où en est un fichier de la liste : formats cibles demandés au cœur. */
export type Analysis =
  /** Format inconnu ou fichier illisible : rien à demander (le motif est dans `info.error`). */
  | { state: "unusable" }
  | { state: "loading" }
  | { state: "ready"; targets: FileTargets }
  | { state: "failed"; error: string };

export const analysisTargets = (a: Analysis): FileTargets | null => (a.state === "ready" ? a.targets : null);

// ------------------------------------------------------------------ tâches de conversion

export type JobStatus = "waiting" | "running" | "done" | "skipped" | "failed" | "cancelled";

/** Transitions permises. Une tâche terminée ne revient jamais en arrière (un événement rejoué
 *  en double ou arrivé en retard est ignoré). */
export const JOB_TRANSITIONS: Readonly<Record<JobStatus, readonly JobStatus[]>> = {
  waiting: ["running", "done", "skipped", "failed", "cancelled"],
  running: ["done", "skipped", "failed", "cancelled"],
  done: [],
  skipped: [],
  failed: [],
  cancelled: [],
};

export const isFinal = (s: JobStatus): boolean => JOB_TRANSITIONS[s].length === 0;
export const isActive = (s: JobStatus): boolean => s === "waiting" || s === "running";

/** Statut suivant si la transition est permise, sinon le statut actuel. */
export function moveJob(from: JobStatus, to: JobStatus): JobStatus {
  return from === to || JOB_TRANSITIONS[from].includes(to) ? to : from;
}

// ------------------------------------------------------------------ mise à jour de l'application

export type UpdateFlow =
  | { state: "idle" }
  | { state: "checking" }
  | { state: "installing"; progress: UpdateProgress | null }
  | { state: "failed"; during: "check" | "install"; error: string };

export type UpdateEvent =
  | { type: "check" }
  | { type: "install" }
  | { type: "progress"; progress: UpdateProgress }
  | { type: "done" }
  | { type: "error"; error: string };

export function updateStep(s: UpdateFlow, e: UpdateEvent): UpdateFlow {
  const busy = s.state === "checking" || s.state === "installing";
  switch (e.type) {
    case "check":
      return busy ? s : { state: "checking" };
    case "install":
      return busy ? s : { state: "installing", progress: null };
    case "progress":
      return s.state === "installing" ? { state: "installing", progress: e.progress } : s;
    case "done":
      return busy ? { state: "idle" } : s;
    case "error":
      return busy ? { state: "failed", during: s.state === "checking" ? "check" : "install", error: e.error } : s;
  }
}

export const updateBusy = (s: UpdateFlow): boolean => s.state === "checking" || s.state === "installing";

// ------------------------------------------------------------------ installation d'un moteur

export type EngineAction = "install" | "uninstall" | "rollback";

/** Opération sur un moteur. Absente = repos. Toute action est d'abord confirmée : on montre ce
 *  qui va être téléchargé (taille, éditeur, licence) ou supprimé, puis l'utilisateur décide. */
export type EngineOp =
  | { state: "confirming"; action: EngineAction }
  | { state: "running"; action: EngineAction; progress: InstallProgress | null; cancelling: boolean }
  | { state: "failed"; action: EngineAction; error: string };

export type EngineOpEvent =
  | { type: "ask"; action: EngineAction }
  | { type: "confirm" }
  | { type: "progress"; progress: InstallProgress }
  | { type: "cancel" }
  | { type: "succeeded" }
  | { type: "error"; error: string }
  | { type: "dismiss" };

/** Actions possibles pour un moteur dans son état actuel (rien n'est proposé pour une
 *  installation du système : Chud Converter ne la modifie jamais). */
export function engineActions(e: EngineView): EngineAction[] {
  const out: EngineAction[] = [];
  const managed = e.origin === "managed";
  const usable = e.state === "ready" || e.state === "no_adapter";
  if (e.installable && (!usable || (managed && e.version !== e.installable.version))) out.push("install");
  if (managed && e.previous) out.push("rollback");
  if (managed) out.push("uninstall");
  return out;
}

export function engineStep(op: EngineOp | null, e: EngineOpEvent): EngineOp | null {
  switch (e.type) {
    case "ask":
      return op?.state === "running" ? op : { state: "confirming", action: e.action };
    case "confirm":
      return op?.state === "confirming" ? { state: "running", action: op.action, progress: null, cancelling: false } : op;
    case "progress":
      return op?.state === "running" ? { ...op, progress: e.progress } : op;
    case "cancel":
      return op?.state === "running" && op.action === "install" ? { ...op, cancelling: true } : op;
    case "succeeded":
      return op?.state === "running" ? null : op;
    case "error":
      return op?.state === "running" ? { state: "failed", action: op.action, error: e.error } : op;
    case "dismiss":
      return op?.state === "running" ? op : null;
  }
}
