# Architecture technique

## 1. Vue d'ensemble

```
┌──────────────────────────── Présentation ────────────────────────────┐
│  Application Tauri 2 (Svelte 5)            CLI `chud`                 │
│  client léger : vues, formulaires générés,  scripts, CI, sortie JSON  │
│  aperçus, éditeur de nœuds                                            │
└───────────────▲───────────────────────────────────▲──────────────────┘
                │ commandes / requêtes / événements typés (serde)
┌───────────────┴──────────── Services (chud-service) ─────────────────┐
│  file d'attente · profils · historique · surveillance · workflows     │
├────────────────────────────── Domaine ───────────────────────────────┤
│ chud-core : catalogue de formats · détection · capacités · planif.   │
│ chud-jobs : staging · exécution des plans · vérification · finalisation│
│ (à venir) chud-inspect · chud-dupes · chud-optimize · chud-workflow   │
├────────────────────────────── Moteurs ───────────────────────────────┤
│ chud-engines : natifs (image, données…) + adaptateurs externes        │
│                (FFmpeg, 7-Zip, LibreOffice, Pandoc, Calibre…)         │
│ (à venir) adaptateurs déclaratifs TOML · plugins WebAssembly          │
├─────────────────────────── Infrastructure ───────────────────────────┤
│ chud-process : processus confinés   chud-enginemgr : détection,       │
│ (Job Objects / groupes, délais,     index signé, téléchargement,      │
│  annulation, sortie bornée)          vérification, installation       │
└───────────────────────────────────────────────────────────────────────┘
```

Règle de dépendance : une couche ne dépend que des couches inférieures. `chud-core` ne dépend
d'aucun moteur ; les moteurs ne connaissent ni l'interface ni la file d'attente.

## 2. Crates

| Crate | Rôle | État |
|---|---|---|
| `chud-core` | Types communs, catalogue de formats (TOML embarqué), détection, trait `Engine`, schéma d'options, registre, planificateur, rapport de fidélité | implémenté |
| `chud-process` | Exécution de processus durcie : arguments séparés, confinement d'arbre, délais, annulation, lecture ligne à ligne (`\n` et `\r`), queue de sortie bornée | implémenté |
| `chud-engines` | Moteurs intégrés : `image` (natif), `data` (natif), `ffmpeg`, `7zip` | implémenté (premier lot) |
| `chud-jobs` | Exécution d'une tâche (plan → staging → étapes → vérification → finalisation), pool de threads, événements, annulation | implémenté |
| `chud-enginemgr` | Manifestes de moteurs, détection (gérés, emplacements connus, PATH), sonde de version ; téléchargement/installation | détection implémentée, installation à venir |
| `chud-cli` | Binaire `chud` | implémenté |
| `chud-service` | Façade applicative commune GUI/CLI (profils, historique SQLite, surveillance) | à venir |
| `chud-inspect`, `chud-dupes`, `chud-optimize`, `chud-workflow`, `chud-plugin-host` | Fonctionnalités avancées | à venir |
| `app/` | Application Tauri + frontend Svelte | à venir (après validation ADR-002) |

## 3. Modèle de formats

`FormatDef` (dans `formats.toml`) :

```toml
[[format]]
id = "webp"
name = "WebP"
category = "image"
extensions = ["webp"]
mime = ["image/webp"]
compression = "both"          # lossless | lossy | both | none
features = ["alpha", "animation", "exif", "xmp", "icc"]
```

- `category` : image, audio, video, document, spreadsheet, presentation, pdf, ebook, archive,
  data, subtitle, font, model3d, geo, other.
- `features` : caractéristiques que le **format peut contenir**. Elles servent à calculer les
  pertes : une caractéristique présente dans la source mais absente du format cible est perdue
  avec certitude ; présente dans les deux mais non déclarée comme conservée par le moteur, elle
  est signalée comme probablement perdue.
- Pseudo-format `directory` : cible des extractions d'archives.

### Détection (`chud_core::detect`)

Ordre et niveaux de confiance :

1. **Signature binaire** (`file-format`, ~150 formats reconnus par octets magiques) → `High`.
2. **Inspection de conteneur** : ZIP → `[Content_Types].xml` (DOCX/XLSX/PPTX), `mimetype`
   (ODT/ODS/ODP/EPUB), etc. → `High`.
3. **Heuristiques texte** (JSON, XML, SVG, YAML, CSV/TSV, SRT, WebVTT, ASS, Markdown, HTML) → `Medium`.
4. **Extension seule** → `Low`, toujours signalée à l'utilisateur.

Le résultat signale aussi une **incohérence d'extension** (« `photo.jpg` est en réalité un PNG »).

## 4. Moteurs

```rust
pub trait Engine: Send + Sync {
    fn info(&self) -> &EngineInfo;               // id, nom, version, licence, origine, état
    fn conversions(&self) -> &[ConversionCap];   // arêtes déclarées (après sonde)
    fn options(&self, from: &FormatId, to: &FormatId) -> Vec<OptionSpec>;
    fn assess(&self, step: &StepQuery) -> StepAssessment;  // fidélité précise selon la sonde
    fn execute(&self, task: &StepTask, ctx: &ExecContext) -> Result<StepOutput, EngineError>;
    fn verify(&self, output: &Path, format: &FormatId, ctx: &ExecContext) -> Verification;
}
```

- `ConversionCap { from, to, fidelity, preserves, cost }` : `fidelity` vaut `Lossless`,
  `Lossy` ou `Depends` (ex. remux possible selon les codecs).
- `OptionSpec` : clé, libellé, type (`Bool`, `Int{min,max}`, `Float`, `Choice`, `Text`),
  valeur par défaut, `advanced` (masquée en mode simple), aide. L'interface génère les
  formulaires à partir de ce schéma ; les profils stockent des `OptionValues`.
- `ExecContext` : jeton d'annulation, émetteur de progression, répertoire de staging, limites
  (délai, mémoire), journal.

Origines d'un moteur : `Builtin` (natif), `Managed` (installé par l'application), `System`
(PATH / gestionnaire de paquets, lecture seule), `Custom` (chemin choisi par l'utilisateur).

## 5. Planification (`chud_core::planner`)

1. Construction du graphe à partir des moteurs **disponibles et activés**.
2. Pour un fichier source (format + caractéristiques réellement présentes si une sonde est
   disponible), recherche de Dijkstra multi-cibles avec coût lexicographique :
   `(étapes avec perte, caractéristiques perdues, nombre d'étapes, coût moteur)`.
3. Longueur maximale : 3 étapes. Les formats intermédiaires sont choisis parmi des formats
   « pivots » sans perte de leur catégorie (PNG, TIFF, WAV, FLAC, MKV, JSON…) ou tout format que
   le graphe propose si aucun pivot n'existe.
4. Pour chaque cible atteignable : meilleur plan + alternatives (autres moteurs).
5. `FidelityReport` : fidélité globale, étapes, intermédiaires, avertissements typés
   (`LossyEncoding`, `FeatureLost`, `FeatureMayBeLost`, `Intermediate`, `Generational`,
   `ContentDependent`, `TargetLimitation`). Jamais « sans perte » si une seule étape ne l'est pas.

## 6. Cycle de vie d'une tâche (`chud-jobs`)

```
Queued → Preparing (détection, sonde, plan) → Running (étape i/n, progression)
       → Verifying → Finalizing → Succeeded
                                ↘ Failed (erreur typée + fin de journal)  ↘ Cancelled
```

1. **Staging** : `<cache>/staging/<job-id>/`, chemins courts et ASCII (`step1.mkv`, `out.mp4`).
   Si l'outil ne gère pas le chemin source (Unicode, longueur), un **lien physique** ASCII est
   créé dans le staging (copie uniquement en dernier recours).
2. **Étapes** : chaque étape lit la sortie de la précédente ; progression globale pondérée.
3. **Vérification** : existence, taille non nulle, re-détection du format, vérification propre
   au moteur (ffprobe : flux et durée ; image : décodage ; archive : test d'intégrité).
4. **Finalisation** : nom final par modèle (`{stem}.{ext}`), politique de conflit
   (`rename` par défaut → `nom (1).ext`, `overwrite`, `skip`), réservation `create_new`,
   déplacement atomique si même volume, sinon copie vers `.partial` puis renommage.
   Dates de modification préservées en option.
5. **Nettoyage** : le staging est supprimé dans tous les cas (garde `Drop`).

Ordonnancement : pool de `N` ouvriers ; chaque tâche a un poids (vidéo = lourd, image = léger)
et la somme des poids actifs est bornée pour ne pas saturer CPU/mémoire/disque.

## 7. Gestionnaire de moteurs (`chud-enginemgr`)

Voir [ENGINE-MANAGER.md](ENGINE-MANAGER.md). Résumé : manifestes déclaratifs (détection,
sources de téléchargement par OS/architecture, vérification), répertoire géré versionné,
pointeur `current`, index signé, refus des sources non vérifiées, jamais de modification des
installations système.

## 8. Communication interface ↔ cœur

- **Commandes** (requête/réponse) : `detect`, `targets`, `plan`, `enqueue`, `cancel`,
  `engines.list`, `engines.install`, `profiles.*`, `history.*`.
- **Événements** (flux) : `JobEvent { id, state, progress, speed, eta, message }` diffusés
  au plus toutes les 100 ms par tâche (lissage côté backend).
- Toutes les structures sont `serde`, exportées en TypeScript ; les erreurs portent un **code**
  stable + paramètres, traduits par l'interface (FR/EN dès le départ).

## 9. Données de l'application

| Donnée | Emplacement (Windows / Linux) | Mode portable |
|---|---|---|
| Configuration, profils, workflows | `%APPDATA%\Chud\` / `~/.config/chud/` | `./data/config` |
| Moteurs gérés | `%LOCALAPPDATA%\Chud\engines\` / `~/.local/share/chud/engines/` | `./data/engines` |
| Staging, cache, miniatures | `%LOCALAPPDATA%\Chud\cache\` / `~/.cache/chud/` | `./data/cache` |
| Historique (SQLite) | dossier de données | `./data` |

## 10. Ajouter un format ou un moteur

- **Format** : une entrée dans `formats.toml` (+ règle de détection si la signature n'est pas
  connue de `file-format`), puis des tests de détection.
- **Moteur intégré** : un module dans `chud-engines` implémentant `Engine`, un manifeste dans
  `chud-enginemgr/data/engines/*.toml` s'il est externe, des tests d'intégration qui produisent
  et **vérifient** des fichiers réels.
- **Moteur déclaratif / plugin** : voir [PLUGINS.md](PLUGINS.md).

## 11. Portabilité

- Code spécifique OS isolé : `chud-process` (confinement), `chud-enginemgr` (emplacements
  connus, noms d'exécutables), intégration shell (à venir).
- macOS préparé : manifestes avec cibles `macos-aarch64`/`macos-x86_64`, groupes de processus
  Unix communs Linux/macOS, chemins via `directories`. Restent à faire : notarisation, bundles.
