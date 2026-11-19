// Miroir des vues sérialisées par src-tauri/src/backend.rs (camelCase).

export type Category =
  | "image"
  | "audio"
  | "video"
  | "document"
  | "spreadsheet"
  | "presentation"
  | "pdf"
  | "ebook"
  | "archive"
  | "data"
  | "subtitle"
  | "font"
  | "model3d"
  | "geo"
  | "other";

/** Fidélité affichée : `partial` = contenu intact mais des informations ne suivent pas. */
export type FidelityLevel = "lossless" | "partial" | "depends" | "lossy";
export type StepFidelity = "lossless" | "depends" | "lossy";

export interface FileInfo {
  path: string;
  name: string;
  size: number;
  isDir: boolean;
  format: string | null;
  formatName: string | null;
  category: Category | null;
  confidence: string | null;
  method: string | null;
  extensionMismatch: boolean;
  note: string | null;
  error: string | null;
}

/** Dossier d'un seul type remplacé par ses fichiers à l'ajout. */
export interface ExpandedFolder {
  folder: string;
  files: string[];
  note: string;
}

export interface Expansion {
  paths: string[];
  folders: ExpandedFolder[];
}

export interface TargetView {
  format: string;
  name: string;
  category: Category;
  extension: string | null;
  fidelity: FidelityLevel;
  headline: string;
  lost: string[];
  engines: string[];
}

/** « Qualité d'origine » : format qui ressort le contenu sans rien perdre. */
export interface OriginalChoice {
  kind: "audio" | "subtitles" | "video" | "image";
  format: string;
  title: string;
  detail: string;
}

export interface FileTargets {
  path: string;
  source: string;
  summary: [string, string][];
  targets: TargetView[];
  original: OriginalChoice[];
}

export type OptionValue = boolean | number | string;

export type OptionKind =
  | { type: "bool" }
  | { type: "int"; min: number; max: number }
  | { type: "float"; min: number; max: number }
  | { type: "choice"; choices: { value: string; label: string }[] }
  | { type: "text"; max_len: number };

export interface OptionSpec {
  key: string;
  label: string;
  help?: string | null;
  kind: OptionKind;
  default: OptionValue;
  advanced: boolean;
}

export interface StepView {
  engine: string;
  from: string;
  to: string;
  fidelity: StepFidelity;
}

export interface PlanView {
  source: string;
  sourceName: string;
  target: string;
  targetName: string;
  fidelity: FidelityLevel;
  headline: string;
  details: string[];
  steps: StepView[];
  options: OptionSpec[];
  output: string;
}

export type EngineState = "ready" | "no_adapter" | "broken" | "missing" | "disabled";

export interface EngineView {
  id: string;
  name: string;
  state: EngineState;
  problem: string | null;
  version: string | null;
  path: string | null;
  origin: string | null;
  license: string;
  homepage: string;
  description: string;
  /** Commande pour la ligne de commande ; l'interface ne l'affiche jamais (tout s'installe d'ici). */
  hint: string | null;
  /** Chud Converter sait se servir de ce moteur (sinon : intégration à venir). */
  supported: boolean;
  /** Familles de fichiers traitées : video, audio, image, archive… */
  categories: string[];
  /** Paquet que Chud Converter sait installer lui-même sur cette plateforme. */
  installable: InstallOffer | null;
  /** Version précédente d'une installation gérée (retour en arrière possible). */
  previous: string | null;
  /** Une installation est en cours (lancée depuis cette fenêtre ou une autre). */
  installing: boolean;
}

export interface InstallOffer {
  version: string;
  /** Octets à télécharger. */
  size: number;
  publisher: string;
  license: string;
  /** Page de la publication, pour vérifier soi-même. */
  page: string;
  /** Windows : le runtime Visual C++ de Microsoft manque et sera installé avec (autorisation demandée). */
  runtime: boolean;
  /** Linux : installation par le gestionnaire de paquets du système (mot de passe demandé). */
  system: { manager: string; packages: string[] } | null;
}

/** Étape d'une installation de moteur (événement « engine-install »). */
export type InstallProgress =
  | { phase: "downloading"; received: number; total: number }
  | { phase: "verifying" }
  /** `total = 0` : avancement inconnu. */
  | { phase: "extracting"; done: number; total: number }
  /** Installation du runtime Visual C++ (Windows affiche sa propre demande d'autorisation). */
  | { phase: "runtime" }
  /** Gestionnaire de paquets du système : dernière ligne affichée. */
  | { phase: "system"; line: string }
  | { phase: "checking" };

/** Nouvelle version publiée. */
export interface AvailableUpdate {
  version: string;
  date: string | null;
  notes: string | null;
  page: string;
}

/** Mises à jour de l'application (écran Réglages). */
export interface UpdateView {
  current: string;
  autoCheck: boolean;
  lastCheck: number | null;
  available: AvailableUpdate | null;
  canInstall: boolean;
  manualReason: string | null;
  releases: string;
  settingsError: string | null;
  /** Opération en cours côté cœur. */
  phase: "idle" | "checking" | "installing";
}

export type UpdateProgress =
  | { phase: "downloading"; received: number; total: number | null }
  | { phase: "verified" }
  | { phase: "installing" };

/** Réglages de l'application (écran Réglages). */
export interface SettingsView {
  language: "system" | "fr" | "en";
  effective: "fr" | "en";
  system: "fr" | "en";
  file: string;
  error?: string;
  menuNote?: string;
  /** L'accueil du premier démarrage a déjà été montré. */
  introSeen: boolean;
}

export interface AppInfo {
  version: string;
  dataDir: string;
  portable: boolean;
  formats: number;
  conversions: number;
  enginesReady: number;
}

export type Conflict = "rename" | "overwrite" | "skip";

export interface PlanRequest {
  path: string;
  target: string;
  options: Record<string, OptionValue>;
}

export interface ConvertRequest {
  files: string[];
  target: string;
  options: Record<string, OptionValue>;
  outputDir: string | null;
  conflict: Conflict;
  keepDate: boolean;
}

export interface Submitted {
  path: string;
  id: number;
}

export type Verification =
  | { level: "decoded"; detail: string }
  | { level: "structural"; detail: string }
  | { level: "signature_only" }
  | { level: "failed"; detail: string };

export type UiEvent =
  | { kind: "started"; id: number }
  | { kind: "planned"; id: number; fidelity: FidelityLevel; headline: string; details: string[] }
  | { kind: "step"; id: number; index: number; count: number; engine: string; from: string; to: string }
  | { kind: "progress"; id: number; fraction: number | null; speed: string | null }
  | { kind: "skipped"; id: number; reason: string }
  | {
      kind: "done";
      id: number;
      output: string | null;
      skipped: boolean;
      fidelity: FidelityLevel | null;
      headline: string | null;
      details: string[];
      verifications: Verification[];
      notes: string[];
      durationMs: number;
    }
  | { kind: "failed"; id: number; code: string; message: string };

/** Sorte de fichier visée par un préréglage : une catégorie, ou `folder` pour un dossier. */
export type PresetKind = Category | "folder";

export interface PresetView {
  id: string;
  name: string;
  target: string;
  /** Rempli par le cœur ; ignoré à l'enregistrement. */
  targetName: string;
  appliesTo: string[];
  options: Record<string, OptionValue>;
  outputDir: string | null;
  nameTemplate: string | null;
  conflict: Conflict;
  keepDate: boolean;
  enabled: boolean;
  /** Fait partie des préréglages fournis. */
  builtin: boolean;
}

export interface PresetsView {
  file: string;
  presets: PresetView[];
  error: string | null;
  /** Après un enregistrement : mise à jour du menu clic droit installé. */
  menuNote?: string | null;
}

export interface Ignored {
  path: string;
  reason: string;
}

export interface RunPreset {
  preset: PresetView;
  submitted: Submitted[];
  ignored: Ignored[];
}

/** Lot lancé par le menu contextuel (`--preset`). */
export interface QuickBatch {
  files: string[];
  run: RunPreset | null;
  error: string | null;
  /** Ce qui est déjà arrivé aux tâches avant l'ouverture de la fenêtre. */
  events: UiEvent[];
}

/** Menu clic droit de l'explorateur de fichiers. */
export interface ShellView {
  supported: boolean;
  installed: boolean;
  exe: string | null;
  exeMissing: boolean;
  /** Le menu lance bien ce programme-ci. */
  current: boolean;
  entries: number;
  location: string;
}
