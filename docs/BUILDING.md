# Compiler, tester, contribuer

## 1. Prérequis

| Outil | Version | Remarque |
|---|---|---|
| Rust | ≥ 1.88 (édition 2024), testé avec 1.99 | `rustup` ; sous Windows la chaîne **MSVC** |
| Windows : Visual Studio Build Tools | 2022 ou plus | charge « Développement Desktop en C++ » (éditeur de liens, `rc.exe` pour l'icône) |
| Linux | gcc ou clang, `pkg-config` | aucune bibliothèque système requise pour la CLI |
| FFmpeg, 7-Zip | facultatifs | nécessaires aux tests réels vidéo/audio et archives (ignorés sinon) |

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
| `chud-cli` | développement des motifs `*.ext` sans réinterpréter un chemin existant |

Les tests qui exigent un moteur absent affichent `SKIP` et réussissent. Avec la variable
`CHUD_REQUIRE_ENGINES=1` (utilisée par la CI), un moteur absent fait **échouer** le test : une
CI verte prouve donc que les tests réels ont bien tourné.

La CI ([`.github/workflows/ci.yml`](../.github/workflows/ci.yml)) tourne sous Windows et Linux à
chaque envoi : installation de FFmpeg et 7-Zip, `cargo fmt --check`, `clippy -D warnings`,
tests.

## 4. Organisation du code

Voir [ARCHITECTURE](ARCHITECTURE.md). En bref : `chud-core` (types, catalogue, détection,
planification, aucun processus), `chud-process` (seul endroit qui lance des programmes),
`chud-engines` (adaptateurs), `chud-jobs` (exécution d'une conversion de bout en bout),
`chud-enginemgr` (moteurs installés), `chud-cli`.

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
