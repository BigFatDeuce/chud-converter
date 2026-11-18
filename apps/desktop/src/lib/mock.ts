// Imitation du cœur pour travailler l'interface dans un navigateur, sans Tauri.
// Chargée uniquement en développement (import.meta.env.DEV) ; l'interface affiche alors un
// bandeau « données fictives ». Rien ici n'est jamais présenté comme un vrai résultat.

import type { Api } from "./api";
import type { Lang, LangChoice } from "./i18n.svelte";
import type { EngineView, FileInfo, FileTargets, PlanView, PresetView, QuickBatch, SettingsView, TargetView, UiEvent, UpdateView } from "./types";

const video = (format: string, name: string, fidelity: TargetView["fidelity"], headline: string, lost: string[] = []): TargetView => ({
  format,
  name,
  category: "video",
  extension: format,
  fidelity,
  headline,
  lost,
  engines: ["ffmpeg"],
});

const FAKE: Record<string, { info: FileInfo; targets: TargetView[] }> = {
  "D:\\Exemple\\vacances.mkv": {
    info: {
      path: "D:\\Exemple\\vacances.mkv",
      name: "vacances.mkv",
      size: 734_003_200,
      isDir: false,
      format: "mkv",
      formatName: "Matroska",
      category: "video",
      confidence: "certaine",
      method: "signature",
      extensionMismatch: false,
      note: null,
      error: null,
    },
    targets: [
      video("mp4", "MPEG-4 Part 14", "depends", "Selon le contenu du fichier", ["sous-titres stylés (ASS)"]),
      video("webm", "WebM", "lossy", "Avec perte"),
      video("mov", "QuickTime", "depends", "Selon le contenu du fichier"),
      { ...video("flac", "FLAC", "partial", "Contenu sans perte, mais certaines informations ne seront pas conservées", ["vidéo"]), category: "audio" },
      { ...video("gif", "GIF", "lossy", "Avec perte", ["son"]), category: "image" },
    ],
  },
  "D:\\Exemple\\faux.jpg": {
    info: {
      path: "D:\\Exemple\\faux.jpg",
      name: "faux.jpg",
      size: 48_213,
      isDir: false,
      format: "png",
      formatName: "PNG",
      category: "image",
      confidence: "certaine",
      method: "signature",
      extensionMismatch: true,
      note: "L'extension .jpg ne correspond pas au contenu (PNG).",
      error: null,
    },
    targets: [
      { format: "webp", name: "WebP", category: "image", extension: "webp", fidelity: "lossy", headline: "Avec perte", lost: [], engines: ["image"] },
      { format: "bmp", name: "BMP", category: "image", extension: "bmp", fidelity: "partial", headline: "Contenu sans perte, mais certaines informations ne seront pas conservées", lost: ["transparence"], engines: ["image"] },
    ],
  },
};

const ENGINES: EngineView[] = [
  {
    id: "ffmpeg",
    name: "FFmpeg",
    state: "ready",
    problem: null,
    version: "0.0 (fictif)",
    path: "C:\\Exemple\\ffmpeg.exe",
    origin: "system",
    license: "LGPL-2.1-or-later / GPL",
    homepage: "https://ffmpeg.org",
    description: "Vidéo, audio, sous-titres (données fictives)",
    hint: null,
  },
  {
    id: "pandoc",
    name: "Pandoc",
    state: "missing",
    problem: null,
    version: null,
    path: null,
    origin: null,
    license: "GPL-2.0-or-later",
    homepage: "https://pandoc.org",
    description: "Documents (données fictives)",
    hint: "winget install JohnMacFarlane.Pandoc",
  },
];

const preset = (id: string, name: string, target: string, targetName: string, appliesTo: string[], options = {}): PresetView => ({
  id,
  name,
  target,
  targetName,
  appliesTo,
  options,
  outputDir: null,
  nameTemplate: null,
  conflict: "rename",
  keepDate: false,
  enabled: true,
  builtin: true,
});

const PRESETS: PresetView[] = [
  preset("mp4", "MP4 (vidéo)", "mp4", "MPEG-4 Part 14", ["video"]),
  preset("mp4-leger", "MP4 léger (720p)", "mp4", "MPEG-4 Part 14", ["video"], { video_quality: "small", max_height: 720 }),
  preset("webm", "WebM", "webm", "WebM", ["video"]),
  preset("mp3", "MP3 192 kbit/s", "mp3", "MP3", ["audio", "video"], { audio_bitrate: 192 }),
  preset("webp", "WebP", "webp", "WebP", ["image"]),
  preset("zip", "ZIP", "zip", "ZIP", ["folder"]),
];

let langChoice: LangChoice = "system";
const langListeners: ((l: Lang) => void)[] = [];
const systemLang = (): Lang => (navigator.language.toLowerCase().startsWith("fr") ? "fr" : "en");
const settings = (): SettingsView => ({
  language: langChoice,
  effective: langChoice === "system" ? systemLang() : langChoice,
  system: systemLang(),
  file: "C:\\Exemple\\config\\engines.toml",
});

let autoUpdate = true;
let checked: number | null = null;
const updateView = (): UpdateView => ({
  current: "0.0.0-fictif",
  autoCheck: autoUpdate,
  lastCheck: checked,
  available: checked ? { version: "9.9.9", date: "2026-11-18T10:00:00Z", notes: "Version fictive (données de démonstration).", page: "https://github.com/BigFatDeuce/chud-converter/releases" } : null,
  canInstall: false,
  manualReason: "version de développement",
  releases: "https://github.com/BigFatDeuce/chud-converter/releases",
  settingsError: null,
});

export function mockApi(): Api {
  let listener: ((e: UiEvent) => void) | null = null;
  let next = 0;
  const timers = new Map<number, ReturnType<typeof setInterval>>();
  const delay = <T>(v: T, ms = 150) => new Promise<T>((r) => setTimeout(() => r(v), ms));
  const emit = (e: UiEvent) => listener?.(e);

  let presets = PRESETS.map((p) => ({ ...p }));
  let quickTaken = false;
  let menu = false;
  const shell = () => ({
    supported: true,
    installed: menu,
    exe: menu ? "C:\\Exemple\\chud-converter.exe" : null,
    exeMissing: false,
    current: menu,
    entries: menu ? 240 : 0,
    location: "registre de l'utilisateur (fictif)",
  });
  const api: Api = {
    fake: true,
    appInfo: () => delay({ version: "0.0.0-fictif", dataDir: "C:\\Exemple", portable: false, formats: 0, conversions: 0, enginesReady: 1 }),
    engines: () => delay(ENGINES),
    inspect: (paths) =>
      delay(
        paths.map(
          (p) =>
            FAKE[p]?.info ?? {
              path: p,
              name: p.split(/[\\/]/).pop() ?? p,
              size: 0,
              isDir: false,
              format: null,
              formatName: null,
              category: null,
              confidence: null,
              method: null,
              extensionMismatch: false,
              note: null,
              error: "format non reconnu",
            },
        ),
      ),
    targets: (path) => {
      const f = FAKE[path];
      if (!f) return Promise.reject("format non reconnu");
      const t: FileTargets = { path, source: f.info.format!, summary: [["Durée", "fictive"]], targets: f.targets };
      return delay(t, 300);
    },
    plan: (req) => {
      const f = FAKE[req.path];
      const t = f?.targets.find((x) => x.format === req.target);
      if (!f || !t) return Promise.reject("conversion impossible (données fictives)");
      const p: PlanView = {
        source: f.info.format!,
        sourceName: f.info.formatName!,
        target: t.format,
        targetName: t.name,
        fidelity: t.fidelity,
        headline: t.headline,
        details: t.lost.map((l) => `Perdu si présent : ${l} (non pris en charge par le format cible)`),
        steps: [{ engine: t.engines[0], from: f.info.format!, to: t.format, fidelity: t.fidelity === "lossy" ? "lossy" : "depends" }],
        options: [
          {
            key: "video_quality",
            label: "Qualité vidéo",
            kind: { type: "choice", choices: [
              { value: "copy", label: "Copier sans réencoder" },
              { value: "high", label: "Haute" },
              { value: "small", label: "Fichier léger" },
            ] },
            default: "copy",
            advanced: false,
          },
          { key: "max_height", label: "Hauteur maximale (pixels, 0 = inchangée)", kind: { type: "int", min: 0, max: 8640 }, default: 0, advanced: true },
          { key: "keep_subtitles", label: "Conserver les sous-titres", kind: { type: "bool" }, default: true, advanced: false },
        ],
        output: req.path.replace(/\.[^.\\/]+$/, `.${t.extension}`),
      };
      return delay(p, 200);
    },
    async convert(req) {
      const out = req.files.map((path) => ({ path, id: ++next }));
      for (const s of out) {
        setTimeout(() => {
          emit({ kind: "started", id: s.id });
          emit({ kind: "planned", id: s.id, fidelity: "depends", headline: "Selon le contenu du fichier", details: [] });
          emit({ kind: "step", id: s.id, index: 0, count: 1, engine: "ffmpeg", from: "mkv", to: req.target });
          let f = 0;
          timers.set(
            s.id,
            setInterval(() => {
              f += 0.07;
              if (f < 1) return emit({ kind: "progress", id: s.id, fraction: f, speed: "2,4x" });
              clearInterval(timers.get(s.id));
              timers.delete(s.id);
              emit({
                kind: "done",
                id: s.id,
                output: s.path.replace(/\.[^.\\/]+$/, `.${req.target}`),
                skipped: false,
                fidelity: "depends",
                headline: "Selon le contenu du fichier",
                details: ["Résultat fictif : aucune conversion n'a eu lieu"],
                verifications: [{ level: "decoded", detail: "fictif" }],
                notes: [],
                durationMs: 4200,
              });
            }, 250),
          );
        }, 50);
      }
      return out;
    },
    initialPaths: () => delay([]),
    async cancel(id) {
      const t = timers.get(id);
      if (!t) return false;
      clearInterval(t);
      timers.delete(id);
      emit({ kind: "failed", id, code: "cancelled", message: "conversion annulée" });
      return true;
    },
    async cancelAll() {
      for (const id of [...timers.keys()]) await this.cancel(id);
    },
    async onQueue(cb) {
      listener = cb;
      return () => (listener = null);
    },
    async onDrop() {
      return () => {};
    },
    pickFiles: () => delay(Object.keys(FAKE)),
    pickFolder: () => delay("D:\\Exemple\\Sorties"),
    async reveal() {},
    async openUrl(url) {
      window.open(url, "_blank", "noopener");
    },
    presets: () => delay({ file: "C:\\Exemple\\presets.toml", presets, error: null }),
    async savePresets(list) {
      presets = list.map((p) => ({ ...p, targetName: p.targetName || p.target.toUpperCase() }));
      return delay({ file: "C:\\Exemple\\presets.toml", presets, error: null });
    },
    async resetPresets() {
      presets = PRESETS.map((p) => ({ ...p }));
      return delay({ view: { file: "C:\\Exemple\\presets.toml", presets, error: null }, backup: "C:\\Exemple\\presets.toml.bak-1" });
    },
    presetsFor: (paths) =>
      delay(presets.filter((p) => p.enabled && paths.every((x) => p.appliesTo.includes(FAKE[x]?.info.category ?? "?")))),
    async runPreset(id, files) {
      const p = presets.find((x) => x.id === id);
      if (!p) throw `préréglage inconnu : ${id}`;
      const submitted = await api.convert({ files, target: p.target, options: p.options, outputDir: null, conflict: p.conflict, keepDate: p.keepDate });
      return { preset: p, submitted, ignored: [] };
    },
    windowKind: async () => (location.hash === "#quick" ? "quick" : "main"),
    async takeQuick() {
      if (quickTaken) return [];
      quickTaken = true;
      const files = Object.keys(FAKE).slice(0, 1);
      const batch: QuickBatch = { files, run: await api.runPreset("mp4", files), error: null, events: [] };
      return [batch];
    },
    async onQuick() {
      return () => {};
    },
    async onOpenFiles() {
      return () => {};
    },
    async showMain() {
      location.hash = "";
      location.reload();
    },
    async closeWindow() {
      window.close();
    },
    shellStatus: () => delay(shell()),
    shellInstall: () => ((menu = true), delay(shell(), 600)),
    shellUninstall: () => ((menu = false), delay(shell(), 300)),
    settings: () => delay(settings()),
    async setLanguage(language) {
      langChoice = language;
      const v = settings();
      for (const cb of langListeners) cb(v.effective);
      return delay(v, 200);
    },
    async onLanguage(cb) {
      langListeners.push(cb);
      return () => {};
    },
    updateStatus: () => delay(updateView()),
    async updateCheck() {
      checked = Math.floor(Date.now() / 1000);
      return delay(updateView(), 700);
    },
    async updateSetAuto(enabled) {
      autoUpdate = enabled;
      return delay(updateView());
    },
    async updateInstall() {
      throw "version de développement";
    },
    async onUpdateAvailable() {
      return () => {};
    },
    async onUpdateProgress() {
      return () => {};
    },
  };
  return api;
}
