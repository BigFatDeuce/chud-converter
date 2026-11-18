# Licences

> Ceci n'est pas un avis juridique. Les points marqués **(à vérifier)** doivent être confirmés
> avant toute distribution publique.

## 1. Principe

1. L'installeur ne contient **que** l'application et ses dépendances Rust/JS (licences
   permissives, inventoriées automatiquement).
2. Les moteurs externes sont **téléchargés à la demande par l'utilisateur, depuis leur source
   officielle**, installés séparément et exécutés comme **processus distincts** communiquant par
   la ligne de commande et des fichiers. Ils ne sont pas liés à l'application (agrégation au
   sens de la GPL).
3. L'écran « Moteurs » affiche pour chacun : licence, source, version, lien vers le code source.

## 2. Moteurs externes et obligations

| Moteur | Licence | Point d'attention |
|---|---|---|
| FFmpeg (build GPL) | GPL-3.0+ (avec x264/x265) | Variante LGPL proposée en option ; **jamais** de build `--enable-nonfree` (fdk-aac). Codecs brevetés (H.264, HEVC, AAC) : responsabilité de l'utilisateur selon sa juridiction, l'application ne redistribue pas d'encodeur. |
| Ghostscript | **AGPL-3.0** ou commercial | Processus séparé uniquement, jamais lié ni modifié. |
| MuPDF | **AGPL-3.0** ou commercial | Idem. |
| 7-Zip | LGPL-2.1 + **restriction unRAR** | Le code unRAR ne peut pas servir à recréer l'algorithme RAR. Aucune création RAR sans WinRAR installé par l'utilisateur. |
| WinRAR (`rar.exe`) | Propriétaire (shareware) | Utilisé uniquement s'il est déjà installé ; jamais téléchargé. |
| LibreOffice | MPL-2.0 | — |
| Pandoc | GPL-2.0+ | — |
| Calibre | GPL-3.0 | Aucune fonction de suppression de DRM. |
| ImageMagick | Licence ImageMagick (type Apache-2.0) | — |
| libvips | LGPL-2.1+ | Les binaires Windows « all » incluent des dépendances dont les licences sont à inventorier **(à vérifier)**. |
| pngquant / libimagequant | GPL-3.0 ou commercial | Intégrable en bibliothèque depuis l'adoption de la GPL-3.0-or-later (ADR-014). |
| Tesseract | Apache-2.0 | Données de langues : Apache-2.0. |
| qpdf | Apache-2.0 | — |
| ExifTool | Artistic / GPL | — |
| MediaInfo | BSD-2-Clause | — |
| DuckDB | MIT | — |
| Typst | Apache-2.0 | — |
| Assimp | BSD-3-Clause | — |
| GDAL | MIT | — |
| Blender, Inkscape, FontForge, GPSBabel, MKVToolNix | GPL | Processus séparés. |
| FluidSynth | LGPL-2.1 | SoundFont : licence propre à vérifier **(à vérifier)**. |

## 3. Dépendances Rust de l'application

Politique : licences compatibles avec la GPL-3.0 (MIT, Apache-2.0, BSD, ISC, Zlib, CC0,
Unicode, MPL-2.0, LGPL, GPL-3.0), contrôlées en CI par `cargo-deny`, et inventaire généré
par `cargo-about` dans `THIRD-PARTY-NOTICES` à chaque version.

Dépendances actuelles du socle : `serde`, `serde_json`, `toml`, `thiserror`, `file-format`,
`image`, `serde_norway`, `csv`, `regex`, `crossbeam-channel`, `directories`, `zip`,
`windows-sys`, `libc`, `clap`, `indicatif`, `anyhow`, `winresource` (compilation seulement) —
toutes MIT et/ou Apache-2.0 (`csv` : Unlicense/MIT).

## 4. Licence de l'application

**GPL-3.0-or-later** (ADR-014), texte complet dans le fichier `LICENSE` à la racine.
La licence du futur SDK de plugins sera décidée avec lui (recommandation : MIT/Apache-2.0).
