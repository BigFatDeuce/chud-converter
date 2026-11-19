# Compiler, tester, contribuer

## 1. Prérequis

| Outil | Version | Remarque |
|---|---|---|
| Rust | ≥ 1.88 (édition 2024), testé avec 1.99 | `rustup` ; sous Windows la chaîne **MSVC** |
| Windows : Visual Studio Build Tools | 2022 ou plus | charge « Développement Desktop en C++ » (éditeur de liens, `rc.exe` pour l'icône) |
| Linux | gcc ou clang, `pkg-config` | aucune bibliothèque système requise pour la CLI |
| Node.js | ≥ 20, testé avec 22 et 26 | pour l'application de bureau uniquement |
| Linux, application de bureau | WebKitGTK 4.1 | Debian/Ubuntu : `sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libssl-dev` |
| FFmpeg, 7-Zip | facultatifs | nécessaires aux tests réels vidéo/audio et archives (ignorés sinon) |
| Pandoc, LibreOffice, Calibre, qpdf, Ghostscript, Tesseract, ImageMagick, libvips, MKVToolNix, MediaInfo, ExifTool | facultatifs | tests réels des adaptateurs (`chud-engines/tests/tools_real.rs`), trouvés dans le `PATH` ou par `CHUD_TOOL_<ID>` (chemin de l'exécutable) ; `CHUD_REQUIRE_TOOLS=1` transforme un outil absent en échec (CI Linux) |

Si `cargo` n'est pas dans le `PATH` après l'installation de rustup (cas observé sur ce poste) :

```bash
export PATH="$HOME/.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin:$PATH"
```

## 2. Compiler

```bash
cargo build --release -p chud-cli
```

Le binaire est `target/release/chud` (`chud.exe` sous Windows, avec l'icône et les informations
de version intégrées par `crates/chud-cli/build.rs`). Si `rc.exe` est introuvable, la compilation
continue sans icône et l'affiche en avertissement.

Profil `release` : LTO léger, une unité de génération, symboles retirés.

### Application de bureau

L'interface (Svelte 5) doit être construite **avant** le programme Rust, qui l'intègre :

```bash
cd apps/desktop
npm ci
npm run check      # vérification des types, aucun avertissement toléré
npm run build      # produit apps/desktop/dist
cd ../..
cargo build --release -p chud-desktop
```

Le programme est `target/release/chud-converter` (`.exe` sous Windows). Il accepte des fichiers
en arguments : `chud-converter vacances.mkv photo.png` les ouvre directement dans la liste.

**Paquet de test portable** : `python scripts/package_test.py` compile tout et prépare
`portable/Chud Converter/` (programmes, fichier `portable`, fichiers d'exemple générés avec
FFmpeg, guide [TESTER](TESTER.md)). Le dossier `portable/` n'est jamais versionné.

Pour travailler l'interface : `npm run dev` dans `apps/desktop` sert l'interface sur
`http://localhost:1420` ; ouverte dans un navigateur, elle utilise une **imitation** du cœur
(bandeau « données fictives » visible) qui n'est jamais incluse dans la version compilée.
`npx tauri dev` lance la vraie application en mode développement.

Comme `chud-desktop` fait partie du workspace, `cargo build --workspace` et
`cargo test --workspace` exigent sous Linux les bibliothèques WebKitGTK ci-dessus et un dossier
`apps/desktop/dist` (construit par `npm run build`).

## 3. Tester

```bash
cargo test --workspace
```

| Suite | Contenu |
|---|---|
| `chud-core` (unitaires) | catalogue, détection (signatures, conteneurs, texte, extension trompeuse), options, planificateur, rapport de fidélité |
| `chud-process` | arguments séparés, noms hostiles, délai, annulation, arbre de processus tué |
| `chud-engines` (unitaires) | recettes FFmpeg et 7-Zip, analyse des listes de capacités, honnêteté des formats d'échantillons et des sous-titres stylés, validation des entrées d'archives |
| `chud-engines/tests/ffmpeg_real.rs` | médias générés par FFmpeg : remultiplexage sans perte vérifié par décodage, WebM avec perte annoncée, FLAC/ALAC identiques à l'octet (MD5), sous-titres cp1252, découpe, nom `-é 文件 test.mkv`, source inchangée |
| `chud-engines/tests/sevenzip_real.rs` | allers-retours zip → 7z → dossier, dossier → tgz → dossier ; refus de zip-slip, chemins absolus, doublons de casse, fausse archive |
| `chud-jobs/tests/jobs_real.rs` | conflits (renommer, ignorer, remplacer), refus d'écraser la source, annulation réelle d'un encodage avec nettoyage, fichier tronqué jamais déclaré réussi |
| `chud-jobs` (file d'attente) | ajout de tâches pendant l'exécution, annulation d'une tâche en attente, un événement de fin par tâche |
| `chud-core` (planificateur) | jamais d'« audio » tiré d'une image ni de sous-titres d'un fichier qui n'en a pas |
| `chud-desktop` | analyse, cibles, plan et vraie conversion PNG → BMP via la file ; préréglages (enregistrement, fichier abîmé jamais écrasé, copie avant rétablissement, lancement) ; arguments `--preset` ; historique rejoué pour la fenêtre rapide ; sérialisation camelCase |
| `chud-jobs` (préréglages) | les 21 préréglages fournis, chacun exécuté sur un vrai fichier ; options reconnues par les moteurs |
| `chud-shell` | contenu des menus d'après les préréglages et les moteurs ; inscription réelle dans une branche de test du registre (Windows), réinstallation sans doublon, retrait exact, clé d'un autre programme intacte, liste trafiquée refusée ; fichiers Dolphin, Nemo, Nautilus |
| `chud-cli` | développement des motifs `*.ext` sans réinterpréter un chemin existant |
| Langues | choix `system`/`fr`/`en` et langue du système ; aucun texte français dans le catalogue anglais ; noms des préréglages fournis traduits, noms personnels conservés ; dans l'application, changement de langue enregistré, appliqué aux rapports et aux préréglages, autres réglages conservés, fichier illisible jamais écrasé |

Les tests qui exigent un moteur absent affichent `SKIP` et réussissent. Avec la variable
`CHUD_REQUIRE_ENGINES=1` (utilisée par la CI), un moteur absent fait **échouer** le test : une
CI verte prouve donc que les tests réels ont bien tourné.

La CI ([`.github/workflows/ci.yml`](../.github/workflows/ci.yml)) tourne sous Windows et Linux à
chaque envoi : installation de FFmpeg et 7-Zip (et de WebKitGTK sous Linux), vérification et
construction de l'interface, `cargo fmt --check`, `clippy -D warnings`, tests.

## 4. Organisation du code

Voir [ARCHITECTURE](ARCHITECTURE.md). En bref : `chud-core` (types, catalogue, détection,
planification, aucun processus), `chud-process` (seul endroit qui lance des programmes),
`chud-engines` (adaptateurs), `chud-jobs` (exécution d'une conversion de bout en bout),
`chud-enginemgr` (moteurs installés), `chud-shell` (menu clic droit), `chud-cli`, et `apps/desktop` (interface Svelte et
`src-tauri`, le pont entre l'interface et le cœur).

## 5. Ajouter un format ou un moteur

- **Format** : une entrée dans `crates/chud-core/data/formats.toml` (identifiant, nom, extensions,
  types MIME, signatures, caractéristiques). Ajouter un test de détection.
- **Moteur externe détecté** : un manifeste `crates/chud-enginemgr/data/engines/<id>.toml`
  (exécutables, arguments de version, expression de version, emplacements connus, commande
  d'installation officielle).
- **Adaptateur** : un module de `chud-engines` qui implémente `Engine` (capacités déclarées,
  `assess` honnête, `execute` avec `chud_process`, `verify` qui relit réellement le résultat),
  plus des tests réels.

## 6. Règles de contribution

1. Jamais de shell ni de ligne de commande construite par concaténation : `ProcessSpec` avec
   arguments séparés et chemins absolus.
2. Une conversion n'est réussie que si le résultat a été **vérifié** ; le code de sortie d'un
   moteur ne suffit pas.
3. Fidélité : en cas de doute, annoncer « avec perte ». Jamais de « sans perte » non démontré.
4. Le fichier source n'est jamais ouvert en écriture.
5. Une fonctionnalité n'est cochée dans la [feuille de route](ROADMAP.md) que lorsqu'elle est
   testée sur des fichiers réels.
6. `cargo fmt`, `cargo clippy --workspace`, `cargo test --workspace` avant toute contribution.
7. Toute nouvelle dépendance ou tout nouveau moteur : licence vérifiée et ajoutée à
   [LICENSES](LICENSES.md).

## Publier une version

1. Mettre la version dans `Cargo.toml` (`[workspace.package] version`) et
   `apps/desktop/package.json`, ajouter sa section à `CHANGELOG.md`.
2. `git tag vX.Y.Z && git push origin vX.Y.Z` : le flux `release.yml` vérifie que le tag et le
   code ont la même version, construit et signe les paquets, ajoute `latest.json` et
   `SHA256SUMS.txt`, puis publie. Une étape en échec laisse un brouillon, jamais une publication.
3. Le secret `TAURI_SIGNING_PRIVATE_KEY` du dépôt contient la clé privée de signature des mises
   à jour (sa clé publique est dans `tauri.conf.json`). Sans elle, plus aucune mise à jour
   automatique n'est possible pour les versions déjà installées.

Construire l'installeur Windows en local :

```bash
cd apps/desktop
TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/chud-converter.key)" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" \
  npx tauri build --bundles nsis --config src-tauri/tauri.bundle-windows.conf.json
```
