// État de l'interface (runes Svelte 5). Le cœur Rust reste la seule source de vérité sur les
// formats, la fidélité et le résultat des conversions : ici, on ne fait qu'afficher.

import { L, i18n, type Lang, type LangChoice } from "./i18n.svelte";
import { api, errorText } from "./api";
import { baseName, fidelityRank } from "./format";
import {
  analysisTargets,
  current,
  installsAlongside,
  engineStep,
  failLoad,
  finishLoad,
  idle,
  isActive,
  isFinal,
  moveJob,
  startLoad,
  updateBusy,
  updateStep,
  type Analysis,
  type EngineAction,
  type EngineOp,
  type EngineOpEvent,
  type JobStatus,
  type Load,
  type UpdateEvent,
  type UpdateFlow,
} from "./machines";
import type {
  AppInfo,
  Category,
  Conflict,
  EngineView,
  FidelityLevel,
  FileInfo,
  OptionValue,
  PresetView,
  QuickBatch,
  SettingsView,
  ShellView,
  Submitted,
  UiEvent,
  UpdateView,
  Verification,
} from "./types";

export type View = "convert" | "presets" | "engines" | "settings" | "about";

export type { JobStatus } from "./machines";

export interface FileEntry {
  info: FileInfo;
  checked: boolean;
  /** Formats cibles : chargement, prêts, échec, ou fichier inutilisable (machine `Analysis`). */
  analysis: Analysis;
}

/** Fichier dont on peut demander les cibles (format reconnu, lisible). */
const usable = (info: FileInfo) => !!info.format && !info.error;

export interface Job {
  id: number;
  path: string;
  name: string;
  target: string;
  targetName: string;
  status: JobStatus;
  fraction: number | null;
  speed: string | null;
  step: { index: number; count: number; engine: string; from: string; to: string } | null;
  fidelity: FidelityLevel | null;
  headline: string | null;
  details: string[];
  output: string | null;
  verifications: Verification[];
  notes: string[];
  error: string | null;
  /** Code stable de l'échec (« verification_failed », « invalid_input »…). */
  errorCode: string | null;
  skipReason: string | null;
  durationMs: number | null;
}

export interface CommonTarget {
  format: string;
  name: string;
  category: Category;
  extension: string | null;
  fidelity: FidelityLevel;
  headline: string;
  lost: string[];
}

/** Cibles possibles pour TOUS les fichiers cochés, avec la fidélité la moins bonne. */
function commonTargets(entries: FileEntry[]): CommonTarget[] {
  const ready = entries.map((e) => analysisTargets(e.analysis)).filter((t) => t !== null);
  if (!ready.length) return [];
  const map = new Map<string, CommonTarget>(
    ready[0].targets.map((t) => [t.format, { ...t, lost: [...t.lost] }]),
  );
  for (const e of ready.slice(1)) {
    const theirs = new Map(e.targets.map((t) => [t.format, t]));
    for (const [key, mine] of map) {
      const t = theirs.get(key);
      if (!t) {
        map.delete(key);
        continue;
      }
      if (fidelityRank[t.fidelity] > fidelityRank[mine.fidelity]) {
        mine.fidelity = t.fidelity;
        mine.headline = t.headline;
      }
      for (const l of t.lost) if (!mine.lost.includes(l)) mine.lost.push(l);
    }
  }
  return [...map.values()];
}

/** Identifiant lisible et unique tiré d'un nom (« MP4 léger » → « mp4-leger »). */
export function presetId(name: string, taken: string[]): string {
  const base =
    name
      .normalize("NFD")
      .replace(/\p{M}/gu, "")
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "")
      .slice(0, 34) || "prereglage";
  let id = base;
  for (let n = 2; taken.includes(id); n++) id = `${base}-${n}`;
  return id;
}

const SETTINGS_KEY = "chud.convert.settings";

interface Saved {
  outputDir: string | null;
  conflict: Conflict;
  keepDate: boolean;
}

function loadSaved(): Saved {
  const fallback: Saved = { outputDir: null, conflict: "rename", keepDate: false };
  try {
    const raw = localStorage.getItem(SETTINGS_KEY);
    return raw ? { ...fallback, ...(JSON.parse(raw) as Partial<Saved>) } : fallback;
  } catch {
    return fallback;
  }
}

class Store {
  view = $state<View>("convert");
  fake = $state(false);
  notice = $state<string | null>(null);

  files = $state<FileEntry[]>([]);
  jobs = $state<Job[]>([]);
  target = $state<string | null>(null);
  /** Seulement les options modifiées par l'utilisateur. */
  optionValues = $state<Record<string, OptionValue>>({});

  outputDir = $state<string | null>(null);
  conflict = $state<Conflict>("rename");
  keepDate = $state(false);
  dropHover = $state(false);

  presets = $state<PresetView[]>([]);
  presetsFile = $state<string | null>(null);
  presetsError = $state<string | null>(null);
  /** Lots du menu contextuel (fenêtre rapide) : erreurs et fichiers ignorés y restent visibles. */
  quick = $state<QuickBatch[]>([]);
  shell = $state<ShellView | null>(null);
  /** Menu clic droit : opération en cours (ajout ou retrait), sinon `null`. */
  shellOp = $state<"installing" | "removing" | null>(null);

  /** Moteurs détectés (machine `Load` : la dernière liste reste affichée pendant une recherche). */
  enginesLoad = $state<Load<EngineView[]>>(idle());
  engines = $derived(current(this.enginesLoad));
  /** Opérations sur les moteurs (installation, retrait, retour en arrière), par identifiant. */
  engineOps = $state<Record<string, EngineOp>>({});
  info = $state<AppInfo | null>(null);
  settings = $state<SettingsView | null>(null);
  update = $state<UpdateView | null>(null);
  /** Recherche ou installation d'une mise à jour (machine `UpdateFlow`). */
  updateFlow = $state<UpdateFlow>({ state: "idle" });
  settingsBusy = $state(false);

  checked = $derived(this.files.filter((f) => f.checked && usable(f.info)));
  loadingTargets = $derived(this.checked.some((f) => f.analysis.state === "loading"));
  common = $derived(commonTargets(this.checked));
  targetInfo = $derived(this.common.find((t) => t.format === this.target) ?? null);
  active = $derived(this.jobs.filter((j) => isActive(j.status)).length);
  /** Sortes des fichiers cochés, au sens des préréglages (catégorie, ou « folder »). */
  checkedKinds = $derived([...new Set(this.checked.map((f) => (f.info.isDir ? "folder" : (f.info.category ?? "?"))))]);
  /** Préréglages applicables à toute la sélection, et réalisables avec les moteurs présents. */
  suggested = $derived(
    this.presets.filter(
      (p) =>
        p.enabled &&
        this.checkedKinds.length > 0 &&
        this.checkedKinds.every((k) => p.appliesTo.includes(k)) &&
        this.common.some((t) => t.format === p.target) &&
        !(Object.keys(p.options).length === 0 && this.checked.every((f) => f.info.format === p.target)),
    ),
  );

  #buffered = new Map<number, UiEvent[]>();
  #noticeTimer: ReturnType<typeof setTimeout> | undefined;

  constructor() {
    const s = loadSaved();
    this.outputDir = s.outputDir;
    this.conflict = s.conflict;
    this.keepDate = s.keepDate;
  }

  /** Langue d'abord, avant d'afficher quoi que ce soit ; puis suivi des changements. */
  async #initLanguage(main: boolean) {
    const a = await api;
    this.settings = await a.settings();
    i18n.lang = this.settings.effective;
    await a.onLanguage((lang) => void this.#languageChanged(lang, main));
  }

  async init() {
    const a = await api;
    this.fake = a.fake;
    await this.#initLanguage(true);
    void this.#initUpdates();
    await a.onEngineInstall((id, progress) => this.#engine(id, { type: "progress", progress }));
    await a.onQueue((e) => this.#onEvent(e));
    await a.onDrop(
      (paths) => {
        this.view = "convert";
        void this.addPaths(paths);
      },
      (over) => (this.dropHover = over),
    );
    await a.onOpenFiles((paths) => {
      this.view = "convert";
      void this.addPaths(paths);
    });
    void this.loadEngines(false);
    void this.loadPresets();
    const initial = await a.initialPaths();
    if (initial.length) await this.addPaths(initial);
  }

  /** Fenêtre rapide : seulement la file des conversions lancées par le menu contextuel. */
  async initQuick() {
    const a = await api;
    this.fake = a.fake;
    await this.#initLanguage(false);
    await a.onQueue((e) => this.#onEvent(e));
    const take = async () => {
      for (const b of await a.takeQuick()) {
        this.quick = [...this.quick, b];
        if (b.run) {
          this.#addJobs(b.run.submitted, b.run.preset.target, b.run.preset.targetName);
          for (const e of b.events) this.#apply(e);
        }
      }
    };
    await a.onQuick(() => void take());
    await take();
  }

  saveSettings() {
    try {
      const s: Saved = { outputDir: this.outputDir, conflict: this.conflict, keepDate: this.keepDate };
      localStorage.setItem(SETTINGS_KEY, JSON.stringify(s));
    } catch {
      // Préférences non mémorisées : sans gravité.
    }
  }

  say(text: string) {
    this.notice = text;
    clearTimeout(this.#noticeTimer);
    this.#noticeTimer = setTimeout(() => (this.notice = null), 6000);
  }

  // ---------------------------------------------------------------- fichiers

  async addPaths(paths: string[]) {
    if (!paths.length) return;
    const a = await api;
    let infos: FileInfo[];
    try {
      infos = await a.inspect(paths);
    } catch (e) {
      this.say(errorText(e));
      return;
    }
    const known = new Set(this.files.map((f) => f.info.path));
    const fresh = infos.filter((i) => !known.has(i.path));
    if (fresh.length < infos.length) {
      const n = infos.length - fresh.length;
      this.say(n === 1 ? L("Ce fichier est déjà dans la liste.", "This file is already in the list.") : L(`${n} fichiers étaient déjà dans la liste.`, `${n} files were already in the list.`));
    }
    for (const info of fresh) {
      const ok = usable(info);
      this.files.push({ info, checked: ok, analysis: ok ? { state: "loading" } : { state: "unusable" } });
    }
    await Promise.all(fresh.filter(usable).map((i) => this.#analyze(i.path)));
    this.#keepTargetValid();
  }

  /** Demande les cibles d'un fichier de la liste (état `loading` → `ready` ou `failed`). */
  async #analyze(path: string) {
    const a = await api;
    let next: Analysis;
    try {
      next = { state: "ready", targets: await a.targets(path) };
    } catch (e) {
      next = { state: "failed", error: errorText(e) };
    }
    const entry = this.files.find((f) => f.info.path === path);
    if (entry) entry.analysis = next;
  }

  async pickFiles() {
    const a = await api;
    try {
      await this.addPaths(await a.pickFiles());
    } catch (e) {
      this.say(errorText(e));
    }
  }

  async pickFolderAsInput() {
    const a = await api;
    try {
      const dir = await a.pickFolder(L("Dossier à convertir (en archive, par exemple)", "Folder to convert (to an archive, for example)"));
      if (dir) await this.addPaths([dir]);
    } catch (e) {
      this.say(errorText(e));
    }
  }

  async pickOutputDir() {
    const a = await api;
    try {
      const dir = await a.pickFolder(L("Dossier de destination", "Destination folder"));
      if (dir) {
        this.outputDir = dir;
        this.saveSettings();
      }
    } catch (e) {
      this.say(errorText(e));
    }
  }

  removeFile(path: string) {
    this.files = this.files.filter((f) => f.info.path !== path);
    this.#keepTargetValid();
  }

  clearFiles() {
    this.files = [];
    this.target = null;
    this.optionValues = {};
  }

  toggle(path: string) {
    const f = this.files.find((x) => x.info.path === path);
    if (f) f.checked = !f.checked;
    this.#keepTargetValid();
  }

  setAllChecked(on: boolean) {
    for (const f of this.files) if (usable(f.info)) f.checked = on;
    this.#keepTargetValid();
  }

  chooseTarget(format: string | null) {
    if (format === this.target) return;
    this.target = format;
    this.optionValues = {};
  }

  setOption(key: string, value: OptionValue, defaultValue: OptionValue) {
    const next = { ...this.optionValues };
    if (value === defaultValue) delete next[key];
    else next[key] = value;
    this.optionValues = next;
  }

  #keepTargetValid() {
    if (this.loadingTargets) return;
    if (this.target && !this.common.some((t) => t.format === this.target)) this.chooseTarget(null);
  }

  // ---------------------------------------------------------------- conversions

  async convert() {
    const target = this.targetInfo;
    if (!target || !this.checked.length) return;
    const a = await api;
    const files = this.checked.map((f) => f.info.path);
    try {
      const submitted = await a.convert({
        files,
        target: target.format,
        options: { ...this.optionValues },
        outputDir: this.outputDir,
        conflict: this.conflict,
        keepDate: this.keepDate,
      });
      this.#addJobs(submitted, target.format, target.name);
    } catch (e) {
      this.say(errorText(e));
    }
  }

  #addJobs(submitted: Submitted[], target: string, targetName: string) {
    {
      const created: Job[] = submitted.map((s) => ({
        id: s.id,
        path: s.path,
        name: baseName(s.path),
        target,
        targetName,
        status: "waiting",
        fraction: null,
        speed: null,
        step: null,
        fidelity: null,
        headline: null,
        details: [],
        output: null,
        verifications: [],
        notes: [],
        error: null,
        errorCode: null,
        skipReason: null,
        durationMs: null,
      }));
      this.jobs = [...created.reverse(), ...this.jobs];
      // Des événements ont pu arriver avant la réponse : on les rejoue dans l'ordre.
      for (const s of submitted) {
        const evs = this.#buffered.get(s.id);
        this.#buffered.delete(s.id);
        for (const e of evs ?? []) this.#apply(e);
      }
    }
  }

  async cancel(id: number) {
    const a = await api;
    await a.cancel(id);
  }

  async cancelAll() {
    const a = await api;
    await a.cancelAll();
  }

  clearFinished() {
    this.jobs = this.jobs.filter((j) => isActive(j.status));
  }

  #onEvent(e: UiEvent) {
    if (!this.jobs.some((j) => j.id === e.id)) {
      const list = this.#buffered.get(e.id) ?? [];
      list.push(e);
      this.#buffered.set(e.id, list);
      return;
    }
    this.#apply(e);
  }

  #apply(e: UiEvent) {
    const j = this.jobs.find((x) => x.id === e.id);
    // Une tâche terminée ne revient jamais en arrière (événements rejoués en double) :
    // les transitions permises sont dans `JOB_TRANSITIONS` (machines.ts).
    if (!j || isFinal(j.status)) return;
    const move = (to: JobStatus) => (j.status = moveJob(j.status, to));
    switch (e.kind) {
      case "started":
        move("running");
        break;
      case "planned":
        j.fidelity = e.fidelity;
        j.headline = e.headline;
        j.details = e.details;
        break;
      case "step":
        j.step = { index: e.index, count: e.count, engine: e.engine, from: e.from, to: e.to };
        j.fraction = null;
        break;
      case "progress":
        j.fraction = e.fraction;
        j.speed = e.speed;
        break;
      case "skipped":
        j.skipReason = e.reason;
        break;
      case "done":
        move(e.skipped ? "skipped" : "done");
        j.output = e.output;
        if (e.fidelity) j.fidelity = e.fidelity;
        if (e.headline) j.headline = e.headline;
        j.details = e.details;
        j.verifications = e.verifications;
        j.notes = e.notes;
        j.durationMs = e.durationMs;
        j.fraction = 1;
        break;
      case "failed":
        move(e.code === "cancelled" ? "cancelled" : "failed");
        j.error = e.message;
        j.errorCode = e.code;
        break;
    }
  }

  // ---------------------------------------------------------------- préréglages

  async loadPresets() {
    const a = await api;
    try {
      const v = await a.presets();
      this.presets = v.presets;
      this.presetsFile = v.file;
      this.presetsError = v.error;
    } catch (e) {
      this.presetsError = errorText(e);
    }
  }

  /** Enregistre la liste complète ; en cas de refus, la liste affichée revient à l'état du disque. */
  async savePresets(list: PresetView[]): Promise<boolean> {
    const a = await api;
    try {
      const v = await a.savePresets(list);
      this.presets = v.presets;
      this.presetsError = v.error;
      if (v.menuNote) this.say(v.menuNote);
      return true;
    } catch (e) {
      this.say(errorText(e));
      await this.loadPresets();
      return false;
    }
  }

  async resetPresets() {
    const a = await api;
    try {
      const r = await a.resetPresets();
      this.presets = r.view.presets;
      this.presetsError = r.view.error;
      const menu = r.view.menuNote ? ` ${r.view.menuNote}` : "";
      this.say((r.backup ? L(`Préréglages rétablis. L'ancien fichier est gardé : ${r.backup}`, `Presets restored. The old file is kept: ${r.backup}`) : L("Préréglages rétablis.", "Presets restored.")) + menu);
    } catch (e) {
      this.say(errorText(e));
    }
  }

  /** Remplit le panneau avec un préréglage : rien n'est converti avant « Convertir ». */
  applyPreset(p: PresetView) {
    if (!this.common.some((t) => t.format === p.target)) return;
    this.target = p.target;
    this.optionValues = { ...p.options };
    this.conflict = p.conflict;
    this.keepDate = p.keepDate;
    this.outputDir = p.outputDir;
    this.say(L(`Préréglage « ${p.name} » appliqué : vérifiez le rapport puis convertissez.`, `Preset “${p.name}” applied: check the report, then convert.`));
  }

  /** Le choix actuel (format, options, destination) devient un préréglage. */
  async saveAsPreset(name: string): Promise<boolean> {
    const t = this.targetInfo;
    const clean = name.trim();
    if (!t || !clean) return false;
    const p: PresetView = {
      id: presetId(clean, this.presets.map((x) => x.id)),
      name: clean,
      target: t.format,
      targetName: t.name,
      appliesTo: this.checkedKinds.filter((k) => k !== "?"),
      options: { ...this.optionValues },
      outputDir: this.outputDir,
      nameTemplate: null,
      conflict: this.conflict,
      keepDate: this.keepDate,
      enabled: true,
      builtin: false,
    };
    const ok = await this.savePresets([p, ...this.presets]);
    if (ok) this.say(L(`Préréglage « ${clean} » enregistré.`, `Preset “${clean}” saved.`));
    return ok;
  }

  // ---------------------------------------------------------------- menu clic droit

  async loadShell() {
    const a = await api;
    try {
      this.shell = await a.shellStatus();
    } catch (e) {
      this.say(errorText(e));
    }
  }

  async setShell(install: boolean) {
    const a = await api;
    if (this.shellOp) return;
    this.shellOp = install ? "installing" : "removing";
    try {
      this.shell = install ? await a.shellInstall() : await a.shellUninstall();
      this.say(install ? L(`Menu clic droit installé (${this.shell.entries} types de fichiers).`, `Right-click menu installed (${this.shell.entries} file types).`) : L("Menu clic droit retiré.", "Right-click menu removed."));
    } catch (e) {
      this.say(errorText(e));
      await this.loadShell();
    } finally {
      this.shellOp = null;
    }
  }

  // ---------------------------------------------------------------- réglages

  async setLanguage(choice: LangChoice) {
    const a = await api;
    this.settingsBusy = true;
    try {
      const v = await a.setLanguage(choice);
      this.settings = v;
      await this.#languageChanged(v.effective, true);
      if (v.menuNote) this.say(v.menuNote);
    } catch (e) {
      this.say(errorText(e));
    } finally {
      this.settingsBusy = false;
    }
  }

  /** Nouvelle langue : l'interface est reconstruite ({#key} dans App.svelte) et les textes
   *  produits par le cœur (formats, cibles, rapports, préréglages, moteurs) sont redemandés.
   *  Les conversions déjà faites gardent leur rapport tel qu'il a été écrit. */
  async #languageChanged(lang: Lang, main: boolean) {
    if (i18n.lang === lang) return;
    i18n.lang = lang;
    if (!main) return;
    const a = await api;
    try {
      this.settings = await a.settings();
    } catch {
      // Affichage seulement.
    }
    void this.loadPresets();
    await this.loadEngines(false);
    await this.#reloadFiles();
  }

  /** Analyse et cibles redemandées, en gardant la sélection, le format et les options. */
  async #reloadFiles() {
    const paths = this.files.map((f) => f.info.path);
    if (!paths.length) return;
    const a = await api;
    let infos: FileInfo[];
    try {
      infos = await a.inspect(paths);
    } catch (e) {
      this.say(errorText(e));
      return;
    }
    await Promise.all(
      infos.map(async (info) => {
        const entry = this.files.find((f) => f.info.path === info.path);
        if (!entry) return;
        entry.info = info;
        if (!usable(info)) {
          entry.analysis = { state: "unusable" };
          entry.checked = false;
          return;
        }
        // Les anciennes cibles restent affichées jusqu'à la réponse (pas de clignotement).
        await this.#analyze(info.path);
      }),
    );
    this.#keepTargetValid();
  }

  // ---------------------------------------------------------------- mises à jour

  async #initUpdates() {
    const a = await api;
    try {
      this.update = await a.updateStatus();
      // Recherche automatique déjà lancée au démarrage : affichée comme en cours.
      if (this.update.phase === "checking") this.#updateEvent({ type: "check" });
      if (this.update.phase === "installing") this.#updateEvent({ type: "install" });
    } catch {
      // Affichage seulement.
    }
    await a.onUpdateAvailable((v) => {
      this.update = v;
      if (this.updateFlow.state === "checking" && v.phase === "idle") this.#updateEvent({ type: "done" });
    });
    await a.onUpdateProgress((progress) => this.#updateEvent({ type: "progress", progress }));
  }

  #updateEvent(e: UpdateEvent) {
    this.updateFlow = updateStep(this.updateFlow, e);
  }

  async checkUpdate() {
    if (updateBusy(this.updateFlow)) return;
    const a = await api;
    this.#updateEvent({ type: "check" });
    try {
      this.update = await a.updateCheck(true);
      this.#updateEvent({ type: "done" });
    } catch (e) {
      this.#updateEvent({ type: "error", error: errorText(e) });
    }
  }

  async setAutoUpdate(enabled: boolean) {
    const a = await api;
    try {
      this.update = await a.updateSetAuto(enabled);
    } catch (e) {
      this.say(errorText(e));
    }
  }

  /** Téléchargement, vérification de la signature, installation ; l'application redémarre. */
  async installUpdate() {
    if (updateBusy(this.updateFlow)) return;
    const a = await api;
    this.#updateEvent({ type: "install" });
    try {
      await a.updateInstall();
      this.#updateEvent({ type: "done" });
    } catch (e) {
      this.#updateEvent({ type: "error", error: errorText(e) });
    }
  }

  // ---------------------------------------------------------------- moteurs

  async loadEngines(refresh: boolean) {
    const a = await api;
    this.enginesLoad = startLoad(this.enginesLoad);
    try {
      const list = await a.engines(refresh);
      this.#enginesLoaded(list);
      if (refresh) await this.#afterEnginesChanged();
      else this.info = await a.appInfo();
    } catch (e) {
      this.enginesLoad = failLoad(this.enginesLoad, errorText(e));
    }
  }

  #enginesLoaded(list: EngineView[]) {
    this.enginesLoad = finishLoad(list);
    // Une installation lancée avant l'ouverture de cette fenêtre : affichée comme en cours.
    for (const e of list) {
      if (e.installing && this.engineOps[e.id]?.state !== "running") {
        this.engineOps[e.id] = { state: "running", action: "install", progress: null, cancelling: false };
      }
    }
  }

  /** Les cibles possibles dépendent des moteurs : elles sont redemandées, en gardant la
   *  sélection, le format choisi et les options. */
  async #afterEnginesChanged() {
    const a = await api;
    try {
      this.info = await a.appInfo();
    } catch {
      // Affichage seulement.
    }
    await this.#reloadFiles();
  }

  #engine(id: string, e: EngineOpEvent) {
    const next = engineStep(this.engineOps[id] ?? null, e);
    if (next) this.engineOps[id] = next;
    else delete this.engineOps[id];
  }

  /** Première étape de toute action : montrer ce qui va se passer et attendre l'accord. */
  askEngine(id: string, action: EngineAction) {
    this.#engine(id, { type: "ask", action });
  }

  dismissEngine(id: string) {
    this.#engine(id, { type: "dismiss" });
  }

  /** L'utilisateur a confirmé : l'action est lancée. */
  async confirmEngine(id: string) {
    const op = this.engineOps[id];
    if (op?.state !== "confirming") return;
    const action = op.action;
    const before = this.engines?.find((e) => e.id === id);
    const name = before?.name ?? id;
    const alongside = !!before && installsAlongside(before);
    this.#engine(id, { type: "confirm" });
    const a = await api;
    try {
      const list =
        action === "install" ? await a.engineInstall(id, alongside) : action === "uninstall" ? await a.engineUninstall(id) : await a.engineRollback(id);
      this.#engine(id, { type: "succeeded" });
      this.#enginesLoaded(list);
      const e = list.find((x) => x.id === id);
      this.say(
        action === "install"
          ? L(`${name} ${e?.version ?? ""} est installé et prêt.`, `${name} ${e?.version ?? ""} is installed and ready.`)
          : action === "uninstall"
            ? L(`${name} a été retiré.`, `${name} was removed.`)
            : L(`${name} est revenu à la version ${e?.version ?? ""}.`, `${name} is back to version ${e?.version ?? ""}.`),
      );
      await this.#afterEnginesChanged();
    } catch (err) {
      const cur = this.engineOps[id];
      if (cur?.state === "running" && cur.cancelling) {
        this.#engine(id, { type: "succeeded" });
        this.say(L(`Installation de ${name} annulée : rien n'a été installé.`, `${name} installation cancelled: nothing was installed.`));
      } else {
        this.#engine(id, { type: "error", error: errorText(err) });
      }
    }
  }

  async cancelEngine(id: string) {
    this.#engine(id, { type: "cancel" });
    const a = await api;
    try {
      await a.engineInstallCancel(id);
    } catch (e) {
      this.say(errorText(e));
    }
  }
}

export const store = new Store();
