// État de l'interface (runes Svelte 5). Le cœur Rust reste la seule source de vérité sur les
// formats, la fidélité et le résultat des conversions : ici, on ne fait qu'afficher.

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
  UiEvent,
  Verification,
} from "./types";

export type View = "convert" | "engines" | "about";

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

  engines = $state<EngineView[] | null>(null);
  enginesLoading = $state(false);
  enginesError = $state<string | null>(null);
  info = $state<AppInfo | null>(null);

  checked = $derived(this.files.filter((f) => f.checked && f.info.format && !f.info.error));
  loadingTargets = $derived(this.checked.some((f) => f.loading));
  common = $derived(commonTargets(this.checked.filter((f) => !f.targetsError)));
  targetInfo = $derived(this.common.find((t) => t.format === this.target) ?? null);
  active = $derived(this.jobs.filter((j) => j.status === "waiting" || j.status === "running").length);

  #buffered = new Map<number, UiEvent[]>();
  #noticeTimer: ReturnType<typeof setTimeout> | undefined;

  constructor() {
    const s = loadSaved();
    this.outputDir = s.outputDir;
    this.conflict = s.conflict;
    this.keepDate = s.keepDate;
  }

  async init() {
    const a = await api;
    this.fake = a.fake;
    await a.onQueue((e) => this.#onEvent(e));
    await a.onDrop(
      (paths) => {
        this.view = "convert";
        void this.addPaths(paths);
      },
      (over) => (this.dropHover = over),
    );
    void this.loadEngines(false);
    const initial = await a.initialPaths();
    if (initial.length) await this.addPaths(initial);
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
      this.say(n === 1 ? "Ce fichier est déjà dans la liste." : `${n} fichiers étaient déjà dans la liste.`);
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
      const dir = await a.pickFolder("Dossier à convertir (en archive, par exemple)");
      if (dir) await this.addPaths([dir]);
    } catch (e) {
      this.say(errorText(e));
    }
  }

  async pickOutputDir() {
    const a = await api;
    try {
      const dir = await a.pickFolder("Dossier de destination");
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
      const created: Job[] = submitted.map((s) => ({
        id: s.id,
        path: s.path,
        name: baseName(s.path),
        target: target.format,
        targetName: target.name,
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
    } catch (e) {
      this.say(errorText(e));
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
    if (!j) return;
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
