// État de l'interface (runes Svelte 5). Le cœur Rust reste la seule source de vérité sur les
// formats, la fidélité et le résultat des conversions : ici, on ne fait qu'afficher.

import { L, i18n, type Lang, type LangChoice } from "./i18n.svelte";
import { api, errorText } from "./api";
import { baseName, fidelityRank } from "./format";
import type {
  AppInfo,
  Category,
  Conflict,
  EngineView,
  FidelityLevel,
  FileInfo,
  FileTargets,
  OptionValue,
  PresetView,
  QuickBatch,
  SettingsView,
  ShellView,
  Submitted,
  UiEvent,
  Verification,
} from "./types";

export type View = "convert" | "presets" | "engines" | "settings" | "about";

export interface FileEntry {
  info: FileInfo;
  checked: boolean;
  targets: FileTargets | null;
  targetsError: string | null;
  loading: boolean;
}

export type JobStatus = "waiting" | "running" | "done" | "skipped" | "failed" | "cancelled";

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
  const ready = entries.filter((e) => e.targets);
  if (!ready.length) return [];
  const map = new Map<string, CommonTarget>(
    ready[0].targets!.targets.map((t) => [t.format, { ...t, lost: [...t.lost] }]),
  );
  for (const e of ready.slice(1)) {
    const theirs = new Map(e.targets!.targets.map((t) => [t.format, t]));
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
  shellBusy = $state(false);

  engines = $state<EngineView[] | null>(null);
  enginesLoading = $state(false);
  enginesError = $state<string | null>(null);
  info = $state<AppInfo | null>(null);
  settings = $state<SettingsView | null>(null);
  settingsBusy = $state(false);

  checked = $derived(this.files.filter((f) => f.checked && f.info.format && !f.info.error));
  loadingTargets = $derived(this.checked.some((f) => f.loading));
  common = $derived(commonTargets(this.checked.filter((f) => !f.targetsError)));
  targetInfo = $derived(this.common.find((t) => t.format === this.target) ?? null);
  active = $derived(this.jobs.filter((j) => j.status === "waiting" || j.status === "running").length);
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
      const usable = !!info.format && !info.error;
      this.files.push({ info, checked: usable, targets: null, targetsError: null, loading: usable });
    }
    await Promise.all(
      fresh
        .filter((i) => i.format && !i.error)
        .map(async (i) => {
          let targets: FileTargets | null = null;
          let error: string | null = null;
          try {
            targets = await a.targets(i.path);
          } catch (e) {
            error = errorText(e);
          }
          const entry = this.files.find((f) => f.info.path === i.path);
          if (entry) {
            entry.targets = targets;
            entry.targetsError = error;
            entry.loading = false;
          }
        }),
    );
    this.#keepTargetValid();
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
    for (const f of this.files) if (f.info.format && !f.info.error) f.checked = on;
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
    this.jobs = this.jobs.filter((j) => j.status === "waiting" || j.status === "running");
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
    // Une tâche terminée ne revient jamais en arrière (événements rejoués en double).
    if (!j || ["done", "skipped", "failed", "cancelled"].includes(j.status)) return;
    switch (e.kind) {
      case "started":
        j.status = "running";
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
        j.status = e.skipped ? "skipped" : "done";
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
        j.status = e.code === "cancelled" ? "cancelled" : "failed";
        j.error = e.message;
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
    this.shellBusy = true;
    try {
      this.shell = install ? await a.shellInstall() : await a.shellUninstall();
      this.say(install ? L(`Menu clic droit installé (${this.shell.entries} types de fichiers).`, `Right-click menu installed (${this.shell.entries} file types).`) : L("Menu clic droit retiré.", "Right-click menu removed."));
    } catch (e) {
      this.say(errorText(e));
      await this.loadShell();
    } finally {
      this.shellBusy = false;
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
        if (!info.format || info.error) return;
        try {
          entry.targets = await a.targets(info.path);
          entry.targetsError = null;
        } catch (e) {
          entry.targetsError = errorText(e);
        }
      }),
    );
  }

  // ---------------------------------------------------------------- moteurs

  async loadEngines(refresh: boolean) {
    const a = await api;
    this.enginesLoading = true;
    this.enginesError = null;
    try {
      this.engines = await a.engines(refresh);
      this.info = await a.appInfo();
      if (refresh) {
        // Les cibles possibles dépendent des moteurs disponibles.
        const paths = this.files.map((f) => f.info.path);
        this.files = [];
        await this.addPaths(paths);
      }
    } catch (e) {
      this.enginesError = errorText(e);
    } finally {
      this.enginesLoading = false;
    }
  }
}

export const store = new Store();
