// Tests des machines à états de l'interface : `npm test` (Node ≥ 22.6, types retirés à la volée).

import { test } from "node:test";
import assert from "node:assert/strict";
import {
  JOB_TRANSITIONS,
  completesInstall,
  current,
  engineActions,
  engineStep,
  failLoad,
  finishLoad,
  idle,
  installsAlongside,
  isFinal,
  missingEngineFor,
  missingEngines,
  moveItem,
  moveJob,
  splitForTool,
  startLoad,
  updateStep,
  type EngineOp,
  type JobStatus,
  type UpdateFlow,
} from "../src/lib/machines.ts";
import type { EngineView } from "../src/lib/types.ts";

test("chargement : la dernière valeur reste affichée pendant un rechargement et après un échec", () => {
  let l = idle<number[]>();
  assert.equal(current(l), null);
  l = startLoad(l);
  assert.equal(l.state, "loading");
  l = finishLoad([1, 2]);
  assert.deepEqual(current(l), [1, 2]);
  l = startLoad(l);
  assert.deepEqual(current(l), [1, 2]);
  l = failLoad(l, "panne");
  assert.equal(l.state, "failed");
  assert.deepEqual(current(l), [1, 2]);
});

test("tâches : un état final ne change plus, aucun retour en arrière", () => {
  const all = Object.keys(JOB_TRANSITIONS) as JobStatus[];
  for (const s of all.filter(isFinal)) for (const t of all) assert.equal(moveJob(s, t), s, `${s} → ${t}`);
  assert.equal(moveJob("waiting", "running"), "running");
  assert.equal(moveJob("running", "waiting"), "running");
  assert.equal(moveJob("waiting", "cancelled"), "cancelled");
  assert.equal(moveJob("running", "done"), "done");
});

test("mise à jour : une seule opération à la fois, l'erreur dit pendant quoi", () => {
  let s: UpdateFlow = { state: "idle" };
  s = updateStep(s, { type: "check" });
  assert.equal(s.state, "checking");
  assert.equal(updateStep(s, { type: "install" }), s, "pas d'installation pendant une recherche");
  s = updateStep(s, { type: "error", error: "hors ligne" });
  assert.deepEqual(s, { state: "failed", during: "check", error: "hors ligne" });
  s = updateStep(s, { type: "install" });
  s = updateStep(s, { type: "progress", progress: { phase: "downloading", received: 5, total: 10 } });
  assert.deepEqual(s, { state: "installing", progress: { phase: "downloading", received: 5, total: 10 } });
  assert.equal(updateStep({ state: "idle" }, { type: "progress", progress: { phase: "verified" } }).state, "idle");
  assert.equal(updateStep(s, { type: "done" }).state, "idle");
});

const engine = (over: Partial<EngineView>): EngineView => ({
  id: "ffmpeg",
  name: "FFmpeg",
  state: "missing",
  problem: null,
  version: null,
  path: null,
  origin: null,
  license: "GPL",
  homepage: "https://ffmpeg.org",
  description: "",
  hint: "winget install Gyan.FFmpeg",
  supported: true,
  categories: ["video", "audio"],
  installable: { version: "9.0.2", size: 114768076, publisher: "gyan.dev", license: "GPL-3.0-or-later", page: "https://example.org" },
  previous: null,
  installing: false,
  missing: [],
  ...over,
});

test("moteurs : actions proposées selon l'état, une installation du système n'est jamais modifiée", () => {
  assert.deepEqual(engineActions(engine({})), ["install"]);
  assert.deepEqual(engineActions(engine({ state: "broken", origin: "system" })), ["install"]);
  assert.deepEqual(engineActions(engine({ state: "ready", origin: "system", version: "7.1" })), ["install"], "copie à côté, sur demande");
  assert.deepEqual(engineActions(engine({ state: "ready", origin: "custom", version: "7.1" })), [], "chemin choisi : prioritaire");
  assert.equal(installsAlongside(engine({ state: "ready", origin: "system" })), true);
  assert.equal(installsAlongside(engine({})), false);
  assert.deepEqual(engineActions(engine({ state: "ready", origin: "managed", version: "9.0.2" })), ["uninstall"]);
  assert.deepEqual(engineActions(engine({ state: "ready", origin: "managed", version: "8.0", previous: "7.1" })), ["install", "rollback", "uninstall"]);
  assert.deepEqual(engineActions(engine({ installable: null })), []);
});

test("moteurs : une installation du système incomplète peut être complétée par ses paquets", () => {
  const calc = { name: "Calc (classeurs)", categories: ["spreadsheet"] };
  const sys = { version: "de votre distribution", size: 0, publisher: "Fedora", license: "MPL-2.0", page: "", runtime: false, system: { manager: "dnf", packages: ["libreoffice-calc"] } };
  const lo = engine({ id: "libreoffice", state: "ready", origin: "system", version: "26.8", categories: ["document", "spreadsheet"], installable: sys, missing: [calc] });
  assert.equal(completesInstall(lo), true);
  assert.equal(installsAlongside(lo), false, "les paquets du système ne se posent pas « à côté »");
  assert.deepEqual(engineActions(lo), ["install"]);
  assert.equal(missingEngineFor("spreadsheet", [lo])?.id, "libreoffice", "un classeur sans conversion propose d'ajouter Calc");
  assert.equal(missingEngineFor("document", [lo]), null, "Writer est là : rien à proposer pour un texte");
  const complete = { ...lo, missing: [], installable: null };
  assert.equal(completesInstall(complete), false);
  assert.deepEqual(engineActions(complete), []);
});

test("moteurs : confirmer avant d'agir, annuler seulement une installation", () => {
  let op: EngineOp | null = null;
  assert.equal(engineStep(op, { type: "confirm" }), null, "rien à confirmer");
  op = engineStep(op, { type: "ask", action: "install" });
  assert.deepEqual(op, { state: "confirming", action: "install" });
  op = engineStep(op, { type: "confirm" });
  assert.equal(op?.state, "running");
  assert.equal(engineStep(op, { type: "dismiss" }), op, "une opération en cours ne disparaît pas");
  assert.equal(engineStep(op, { type: "ask", action: "uninstall" }), op);
  op = engineStep(op, { type: "progress", progress: { phase: "downloading", received: 1, total: 2 } });
  op = engineStep(op, { type: "cancel" });
  assert.equal(op?.state === "running" && op.cancelling, true);
  assert.deepEqual(engineStep(op, { type: "error", error: "x" }), { state: "failed", action: "install", error: "x" });
  assert.equal(engineStep(op, { type: "succeeded" }), null);
  const un = engineStep(engineStep(null, { type: "ask", action: "uninstall" }), { type: "confirm" });
  assert.equal(engineStep(un, { type: "cancel" }), un, "une suppression ne s'annule pas à moitié");
});

test("fichier sans conversion : le moteur à installer est proposé seulement s'il sert et s'installe d'ici", () => {
  const ff = engine({});
  const doc = engine({ id: "pandoc", name: "Pandoc", supported: false, categories: ["document"] });
  assert.equal(missingEngineFor("video", [doc, ff])?.id, "ffmpeg");
  assert.equal(missingEngineFor("document", [doc, ff]), null, "pas encore pris en charge : rien à installer");
  assert.equal(missingEngineFor("archive", [ff]), null);
  assert.equal(missingEngineFor(null, [ff]), null);
  assert.equal(missingEngineFor("video", null), null, "moteurs pas encore chargés");
  assert.equal(missingEngineFor("video", [engine({ state: "ready", origin: "system" })]), null, "déjà prêt");
  assert.equal(missingEngineFor("video", [engine({ installable: null })]), null, "pas de source ici");
  assert.equal(missingEngineFor("video", [engine({ state: "disabled" })]), null, "désactivé par l'utilisateur");
});

test("tout installer : seulement les moteurs utilisés, absents ou en panne, installables ici", () => {
  const list = [
    engine({}),
    engine({ id: "7zip", name: "7-Zip", state: "broken", origin: "system", categories: ["archive"] }),
    engine({ id: "pandoc", supported: false }),
    engine({ id: "sys", state: "ready", origin: "system" }),
    engine({ id: "nosrc", installable: null }),
    engine({ id: "off", state: "disabled" }),
  ];
  assert.deepEqual(missingEngines(list).map((e) => e.id), ["ffmpeg", "7zip"]);
  assert.deepEqual(missingEngines(null), []);
});

test("animations : le réglage explicite l'emporte, « automatique » suit le système", async () => {
  const { motionLevel } = await import("../src/lib/machines.ts");
  assert.equal(motionLevel("auto", false), "full");
  assert.equal(motionLevel("auto", true), "reduced");
  assert.equal(motionLevel("reduced", false), "reduced");
  assert.equal(motionLevel("off", false), "off");
  assert.equal(motionLevel("off", true), "off");
});

test("outils : les fichiers d'un autre format sont laissés de côté, l'archive prend tout", () => {
  const pdf = { format: "pdf", isDir: false };
  const png = { format: "png", isDir: false };
  const dir = { format: "directory", isDir: true };
  const unknown = { format: null, isDir: false };
  const all = [png, pdf, dir, unknown];
  assert.deepEqual(splitForTool(["pdf"], all), { ok: [pdf], aside: [png, dir, unknown] });
  assert.deepEqual(splitForTool([], all), { ok: all, aside: [] });
});

test("outils : l'ordre de la fusion change d'un cran, jamais hors limites", () => {
  assert.deepEqual(moveItem(["a", "b", "c"], 2, -1), ["a", "c", "b"]);
  assert.deepEqual(moveItem(["a", "b", "c"], 0, 1), ["b", "a", "c"]);
  assert.deepEqual(moveItem(["a", "b"], 0, -1), ["a", "b"]);
  assert.deepEqual(moveItem(["a", "b"], 1, 1), ["a", "b"]);
  assert.deepEqual(moveItem(["a"], -1, 1), ["a"]);
});
