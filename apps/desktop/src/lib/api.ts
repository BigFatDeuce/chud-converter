// Accès au cœur Rust. Dans l'application, tout passe par les commandes Tauri ; dans un simple
// navigateur (développement de l'interface uniquement), une imitation affiche des données
// fictives, clairement signalées. Cette imitation n'existe pas dans la version compilée.

import { L, type Lang, type LangChoice } from "./i18n.svelte";
import type {
  AppInfo,
  ConvertRequest,
  EngineView,
  FileInfo,
  FileTargets,
  PlanRequest,
  PlanView,
  PresetView,
  PresetsView,
  QuickBatch,
  RunPreset,
  ShellView,
  Submitted,
  SettingsView,
  UiEvent,
} from "./types";

/** Fenêtre où tourne l'interface : principale, ou petite fenêtre du menu contextuel. */
export type WindowKind = "main" | "quick";

export interface Api {
  /** Vrai seulement pour l'imitation de développement. */
  readonly fake: boolean;
  appInfo(): Promise<AppInfo>;
  engines(refresh: boolean): Promise<EngineView[]>;
  inspect(paths: string[]): Promise<FileInfo[]>;
  targets(path: string): Promise<FileTargets>;
  plan(request: PlanRequest): Promise<PlanView>;
  convert(request: ConvertRequest): Promise<Submitted[]>;
  initialPaths(): Promise<string[]>;
  cancel(id: number): Promise<boolean>;
  cancelAll(): Promise<void>;
  onQueue(cb: (e: UiEvent) => void): Promise<() => void>;
  onDrop(drop: (paths: string[]) => void, hover: (over: boolean) => void): Promise<() => void>;
  pickFiles(): Promise<string[]>;
  pickFolder(title: string): Promise<string | null>;
  reveal(path: string): Promise<void>;
  openUrl(url: string): Promise<void>;

  presets(): Promise<PresetsView>;
  savePresets(presets: PresetView[]): Promise<PresetsView>;
  /** Rétablit les préréglages fournis ; l'ancien fichier est gardé (`backup`). */
  resetPresets(): Promise<{ view: PresetsView; backup: string | null }>;
  presetsFor(paths: string[]): Promise<PresetView[]>;
  runPreset(id: string, files: string[]): Promise<RunPreset>;

  windowKind(): Promise<WindowKind>;
  takeQuick(): Promise<QuickBatch[]>;
  /** Prévient quand un lot du menu contextuel attend d'être affiché. */
  onQuick(cb: () => void): Promise<() => void>;
  /** Fichiers transmis par une autre instance (« Ouvrir avec » pendant que l'appli tourne). */
  onOpenFiles(cb: (paths: string[]) => void): Promise<() => void>;
  showMain(): Promise<void>;
  closeWindow(): Promise<void>;

  shellStatus(): Promise<ShellView>;
  shellInstall(): Promise<ShellView>;
  shellUninstall(): Promise<ShellView>;

  settings(): Promise<SettingsView>;
  setLanguage(language: LangChoice): Promise<SettingsView>;
  /** La langue a changé (depuis n'importe quelle fenêtre). */
  onLanguage(cb: (lang: Lang) => void): Promise<() => void>;
}

function tauriApi(): Api {
  const core = import("@tauri-apps/api/core");
  const call = async <T>(cmd: string, args?: Record<string, unknown>) => (await core).invoke<T>(cmd, args);
  return {
    fake: false,
    appInfo: () => call("app_info"),
    engines: (refresh) => call("engines", { refresh }),
    inspect: (paths) => call("inspect", { paths }),
    targets: (path) => call("targets", { path }),
    plan: (request) => call("plan", { request }),
    convert: (request) => call("convert", { request }),
    initialPaths: () => call("initial_paths"),
    cancel: (id) => call("cancel", { id }),
    cancelAll: () => call("cancel_all"),
    async onQueue(cb) {
      const { listen } = await import("@tauri-apps/api/event");
      return listen<UiEvent>("queue", (e) => cb(e.payload));
    },
    async onDrop(drop, hover) {
      const { getCurrentWebview } = await import("@tauri-apps/api/webview");
      return getCurrentWebview().onDragDropEvent((e) => {
        const p = e.payload;
        if (p.type === "enter" || p.type === "over") hover(true);
        else if (p.type === "leave") hover(false);
        else if (p.type === "drop") {
          hover(false);
          if (p.paths.length) drop(p.paths);
        }
      });
    },
    async pickFiles() {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const r = await open({ multiple: true, directory: false, title: L("Fichiers à convertir", "Files to convert") });
      return r ?? [];
    },
    async pickFolder(title) {
      const { open } = await import("@tauri-apps/plugin-dialog");
      return await open({ multiple: false, directory: true, title });
    },
    async reveal(path) {
      const { revealItemInDir } = await import("@tauri-apps/plugin-opener");
      await revealItemInDir(path);
    },
    async openUrl(url) {
      const { openUrl } = await import("@tauri-apps/plugin-opener");
      await openUrl(url);
    },
    presets: () => call("presets"),
    savePresets: (presets) => call("save_presets", { presets }),
    resetPresets: () => call("reset_presets"),
    presetsFor: (paths) => call("presets_for", { paths }),
    runPreset: (id, files) => call("run_preset", { id, files }),
    async windowKind() {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      return getCurrentWindow().label === "quick" ? "quick" : "main";
    },
    takeQuick: () => call("take_quick"),
    async onQuick(cb) {
      const { listen } = await import("@tauri-apps/api/event");
      return listen("quick", () => cb());
    },
    async onOpenFiles(cb) {
      const { listen } = await import("@tauri-apps/api/event");
      return listen<string[]>("open-files", (e) => cb(e.payload));
    },
    showMain: () => call("show_main"),
    shellStatus: () => call("shell_status"),
    shellInstall: () => call("shell_install"),
    shellUninstall: () => call("shell_uninstall"),
    settings: () => call("settings"),
    setLanguage: (language) => call("set_language", { language }),
    async onLanguage(cb) {
      const { listen } = await import("@tauri-apps/api/event");
      return listen<Lang>("language", (e) => cb(e.payload));
    },
    async closeWindow() {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      await getCurrentWindow().close();
    },
  };
}

async function load(): Promise<Api> {
  if ("__TAURI_INTERNALS__" in window) return tauriApi();
  if (import.meta.env.DEV) return (await import("./mock")).mockApi();
  throw new Error(L("Chud Converter doit être lancé comme application de bureau.", "Chud Converter must be run as a desktop app."));
}

export const api: Promise<Api> = load();

/** Message lisible pour une erreur renvoyée par une commande. */
export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}
