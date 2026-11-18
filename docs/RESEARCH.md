# Étude technique préalable

> Document de recherche de la phase 1 (novembre 2026). Il sert de base aux décisions
> consignées dans [DECISIONS.md](DECISIONS.md). Les points marqués **(à vérifier)**
> doivent être confirmés au moment de l'intégration du composant concerné, car ils
> dépendent de versions ou de politiques de distribution qui évoluent.

## 1. Environnement de développement constaté

| Outil | Version | Remarque |
|---|---|---|
| Rust (stable, MSVC) | 1.99.1 | toolchain présente mais `~/.cargo/bin` absent du PATH |
| Visual Studio Build Tools | 18 (MSVC 14.51) | éditeur de liens disponible |
| Node.js / npm | 26.13 / 11.20 | suffisant pour le frontend |
| FFmpeg / ffprobe | 9.0.2 (build gyan.dev « full », GPLv3) | sert aux tests d'intégration |
| 7-Zip | 26.06 | `C:\Program Files\7-Zip\7z.exe` |
| MKVToolNix, HandBrake, WinRAR, File Converter | installés | utiles pour la détection de moteurs et l'étude des concurrents |

## 2. Technologie d'interface

Critères : qualité visuelle atteignable, richesse des composants (liste virtualisée de
milliers de fichiers, éditeur de nœuds, aperçus image/vidéo/PDF, glisser-déposer, comparateur
avant/après), accessibilité, internationalisation, packaging Windows/Linux, mise à jour de
l'application, maturité.

| Critère | **Tauri 2 + frontend web** | Slint | Iced | egui |
|---|---|---|---|---|
| Qualité/finition UI atteignable | Excellente (CSS, animations, typographie) | Bonne, déclarative | Correcte, thèmes limités | Aspect « outil », peu adapté au grand public |
| Composants disponibles | Immenses (xyflow/Svelte Flow pour les nœuds, pdf.js, listes virtualisées, graphiques) | À construire soi-même | À construire, peu d'écosystème | egui_node_graph / egui-snarl, rendu brut |
| Aperçus médias | Image/vidéo natifs du WebView (H.264/VP9/AV1 sous WebView2), pdf.js | Décodage à faire soi-même | Idem | Idem |
| Accessibilité | Celle de la plateforme web (la meilleure) | AccessKit | Partielle | AccessKit |
| Démarrage / fluidité | Très bon sous Windows (WebView2 partagé), correct sous Linux | Excellent | Excellent | Excellent |
| Linux | WebKitGTK : moins performant, lecture vidéo dépendante de GStreamer | Natif | Natif (wgpu) | Natif |
| Packaging + mises à jour | Intégrés : NSIS, MSI, AppImage, deb, rpm + updater signé Ed25519 | cargo-packager à ajouter | Idem | Idem |
| Licence | MIT/Apache-2.0 | GPLv3 / royalty-free avec attribution / commerciale | MIT | MIT/Apache-2.0 |

Points relevés pendant la recherche :

- Tauri 2.10/2.12 (sept.–oct. 2026) : les AppImage utilisent désormais Wayland nativement,
  l'updater gère deb/rpm/AppImage/NSIS/MSI, et la vérification minisign rejette les signatures
  Ed25519 non canoniques.
- Le comparatif de performances le plus cité date de 2023 et place Tauri légèrement derrière
  Iced/egui au démarrage : écart non perceptible pour une application de productivité.
- Svelte Flow (`@xyflow/svelte`, 1.6.x en 2026) est réécrit pour Svelte 5 : base idéale pour
  l'éditeur de workflows.

**Conclusion** : Tauri 2 + Svelte 5 + TypeScript (voir ADR-002). Le risque principal (WebKitGTK
sous Linux) est maîtrisé en ne confiant jamais au WebView le décodage des médias inconnus : les
aperçus et miniatures sont produits par le backend (FFmpeg, libvips, pdfium) dans des formats
sûrs (WebP/PNG, H.264 proxy court).

## 3. Moteurs de conversion par catégorie

Légende licences : les moteurs externes sont **appelés comme processus séparés** et
**téléchargés à la demande depuis leur source officielle** ; ils ne sont jamais liés au code de
l'application. Les tailles sont des ordres de grandeur **(à vérifier)** dans les manifestes.

### Images
| Moteur | Rôle | Licence | Notes |
|---|---|---|---|
| `image` (crate Rust) + `fast_image_resize` | Formats courants en natif, redimensionnement | MIT/Apache | PNG, JPEG, GIF, WebP (décodage, encodage sans perte), BMP, ICO, TIFF, TGA, PNM, QOI, HDR, EXR, DDS (lecture) |
| libvips | Conversion rapide et économe en mémoire, très gros fichiers | LGPL-2.1+ | HEIF/AVIF, JXL, JP2, TIFF pyramidal, PDF/SVG via délégués ; binaires Windows `build-win64-mxe` |
| ImageMagick 7 | Couverture maximale (200+ formats), CMJN, PSD/XCF en lecture | Licence ImageMagick (type Apache) | `policy.xml` durci obligatoire (cf. SECURITY) |
| libjxl, libavif, libwebp, OpenJPEG, libheif (outils CLI) | Encodeurs de référence | BSD / LGPL-3 (libheif) | meilleure qualité qu'un encodeur générique |
| oxipng, mozjpeg/jpegtran, gifsicle, pngquant | Optimisation | MIT / BSD / GPL-2 / GPL-3 | pngquant et gifsicle uniquement en processus séparé |
| resvg | SVG → raster, natif | Apache/MIT | moteur de rendu SVG le plus conforme en Rust |
| Inkscape CLI | Vectoriel : SVG ↔ PDF/EPS/PS/EMF/WMF, import AI | GPL-2+ | lourd (~120 Mo), optionnel |
| LibRaw / `rawler` (dnglab) | RAW (CR2/CR3/NEF/ARW/DNG/RAF…) | LGPL-2.1 | développement « fidèle » via darktable-cli/RawTherapee en option avancée |
| `image_dds`, texconv, basisu | Textures (DDS BCn, KTX2) | MIT / MIT / Apache | ressources de jeux |

### Audio
| Moteur | Rôle | Licence | Notes |
|---|---|---|---|
| FFmpeg | Moteur principal (quasi tous codecs/conteneurs) | LGPL-2.1+ ou GPL selon build | build BtbN/gyan téléchargé à la demande ; jamais de build « nonfree » |
| `symphonia` | Décodage natif (inspection, empreintes) | MPL-2.0 | |
| `lofty` | Lecture/écriture des tags (ID3, Vorbis, APE, MP4…) | MIT/Apache | pour **conserver** les métadonnées que FFmpeg perd |
| FluidSynth + SoundFont | MIDI → audio | LGPL-2.1 | SoundFont GeneralUser GS ou FluidR3 (licences à vérifier) |
| libopenmpt / vgmstream | Formats tracker et audio de jeux (centaines de formats rares) | BSD-3 / type ISC | |
| `rusty-chromaprint` / fpcalc | Empreintes acoustiques (doublons audio) | MIT / LGPL | |
| `ebur128` | Mesure de sonie EBU R128 | MIT | normalisation en deux passes avec FFmpeg `loudnorm` |

### Vidéo
| Moteur | Rôle | Licence | Notes |
|---|---|---|---|
| FFmpeg / ffprobe | Conteneurs, codecs, filtres, extraction, sous-titres texte | LGPL/GPL | x264/x265 uniquement dans les builds GPL |
| Accélération matérielle | NVENC, QSV, AMF, VAAPI, Vulkan | — | détectée par **encodage-test réel**, pas par la seule liste `-encoders` |
| MKVToolNix | Matroska avancé (pistes, chapitres, pièces jointes) | GPL-2 | |
| MediaInfo CLI | Inspection détaillée (HDR, Dolby Vision, profils) | BSD-2 | |
| ab-av1 / av1an | Recherche de CRF par cible VMAF | MIT / GPL-3 | mode optimisation |
| libvmaf (dans FFmpeg) | Mesure de qualité VMAF/SSIM/PSNR | BSD-2+patent | |
| Subtitle Edit CLI | Sous-titres image (PGS/VobSub) → texte par OCR | GPL-3 | Windows ; Tesseract sous Linux |
| whisper.cpp | Génération de sous-titres par reconnaissance vocale locale | MIT | idée de fonctionnalité (phase 4) |

### Documents, PDF, OCR
| Moteur | Rôle | Licence | Notes |
|---|---|---|---|
| LibreOffice headless | DOC/DOCX/ODT/RTF/XLS/XLSX/ODS/PPT/PPTX/ODP → PDF et entre eux ; nombreux formats anciens (WordPerfect, Works, Visio, Publisher, Pages/Numbers/Keynote, QuarkXPress…) | MPL-2.0 | ~350 Mo ; un profil utilisateur isolé par tâche pour la parallélisation |
| Pandoc | Formats balisés : MD, HTML, DOCX, ODT, EPUB, LaTeX, RST, Org, AsciiDoc, Typst, DocBook, ipynb… | GPL-2+ | option `--sandbox` |
| Typst | Production de PDF depuis Markdown/Pandoc sans LaTeX | Apache-2.0 | binaire unique et léger |
| qpdf | Structure PDF, linéarisation, fusion/division, chiffrement avec mot de passe, réparation | Apache-2.0 | |
| pdfium (`pdfium-render`) | Rendu de pages, extraction de texte, natif | BSD/Apache | binaires bblanchon/pdfium-binaries |
| `lopdf` | Manipulation PDF native (rotation, pages, filigrane, métadonnées) | MIT | |
| Ghostscript | PS/EPS, compression PDF, PDF/A | **AGPL-3** ou commercial | processus séparé uniquement |
| MuPDF (mutool) | Rendu, nettoyage, réparation | **AGPL-3** ou commercial | alternative à Ghostscript |
| Tesseract | OCR (100+ langues) | Apache-2.0 | sortie PDF consultable native (`tesseract … pdf`) |
| OCRmyPDF | Pipeline OCR → PDF/A | MPL-2.0 | dépend de Python ; on reproduira le pipeline en Rust avec pdfium + Tesseract + qpdf |
| `calamine` / `rust_xlsxwriter` | Tableurs natifs (lecture XLS/XLSX/ODS, écriture XLSX) | MIT / MIT-Apache | évite LibreOffice pour tableur ↔ données |

### Livres électroniques
| Moteur | Rôle | Licence | Notes |
|---|---|---|---|
| Calibre (`ebook-convert`, `ebook-meta`, `ebook-polish`) | Référence absolue : EPUB, MOBI, AZW3, FB2, LIT, PDB, LRF, DOCX, PDF… | GPL-3 | ~200 Mo ; **aucune suppression de DRM** |
| Pandoc | Création d'EPUB depuis MD/DOCX/HTML | GPL-2+ | |
| Natif | CBZ/CB7/CBT ↔ images ↔ PDF | — | CBR via 7-Zip/unRAR (lecture seule) |
| epubcheck | Validation EPUB | BSD-3 (Java) | optionnel |

### Archives
| Moteur | Rôle | Licence | Notes |
|---|---|---|---|
| 7-Zip (`7z`/`7zz`) | Lecture de ~50 formats (7z, ZIP, RAR, CAB, ISO, WIM, VHD, DMG, DEB, RPM, MSI, SquashFS…), écriture 7z/ZIP/TAR/GZ/BZ2/XZ/WIM | LGPL-2.1 + restriction unRAR | Linux : binaires officiels `7zz` |
| Crates natives (`zip`, `tar`, `flate2`, `xz`, `bzip2`, `zstd`, `lz4_flex`, `sevenz-rust2`) | Extraction **contrôlée** (anti zip-bomb, anti traversée) et création | MIT/Apache | utilisées en priorité quand le format le permet |
| WinRAR `rar.exe` | Création RAR | Propriétaire | **uniquement s'il est déjà installé** ; jamais téléchargé ni redistribué |
| unar/lsar | Formats anciens (StuffIt, LZH, ARC, ZOO…) | LGPL-2.1 | |
| xorriso | Création ISO | GPL-3 | |

### Données structurées
| Moteur | Rôle | Licence |
|---|---|---|
| Natif (serde_json, serde_norway (YAML), toml, csv, quick-xml) | JSON, YAML, TOML, CSV/TSV, NDJSON, XML | MIT/Apache |
| `encoding_rs` + `chardetng` | Détection et conversion d'encodages (UTF-8/16, Windows-125x, ISO-8859-x, Shift-JIS…) | MIT/Apache |
| DuckDB CLI | CSV ↔ Parquet ↔ JSON ↔ SQLite ↔ XLSX, requêtes, nettoyage | MIT |
| `rusqlite` (SQLite embarqué) | SQLite, historique interne | MIT / domaine public |
| `arrow`/`parquet` | Parquet natif | Apache-2.0 |

### Autres catégories étudiées
| Catégorie | Moteurs pertinents | Licences |
|---|---|---|
| 3D | Assimp (40+ formats en lecture), gltf-transform/gltfpack (optimisation glTF), Blender CLI (`.blend`), OpenCascade/FreeCAD (STEP/IGES) | BSD-3, MIT, GPL, LGPL |
| Polices | fontTools (TTF/OTF/WOFF/WOFF2, TTX), FontForge (formats anciens), `ttf-parser`/`allsorts` (inspection) | MIT, GPL, MIT/Apache |
| Sous-titres | Natif (SRT, WebVTT, ASS/SSA, MicroDVD, SBV, TTML, LRC), FFmpeg, Subtitle Edit | — |
| Géographie | GDAL/OGR (Shapefile, GeoJSON, KML, GPX, GeoPackage, GeoTIFF…), GPSBabel (GPS) | MIT, GPL |
| Scientifique | GDAL (HDF5/NetCDF), dcmtk (DICOM), FITS via libvips | MIT, BSD |
| E-mails | `mail-parser` (EML/MBOX), MSG (à étudier) | MIT/Apache |
| Inspection/métadonnées | ExifTool (référence, ~400 formats), MediaInfo, ffprobe | Artistic/GPL, BSD-2 |

## 4. Logiciels concurrents et leçons retenues

| Logiciel | Points forts | Limites | Ce qu'on retient |
|---|---|---|---|
| File Converter (Tichau, GPL-3, Windows) | Conversion depuis le **menu contextuel de l'Explorateur**, préréglages | Peu d'options, Windows seulement | Intégration shell (Explorateur, Nautilus, Dolphin) |
| Shutter Encoder (Java, GPL) | Énormément de fonctions vidéo | Interface dense et datée | Puissance, mais avec divulgation progressive |
| HandBrake | Préréglages, file d'attente exemplaire | Vidéo uniquement | Modèle de file d'attente et de préréglages |
| XnConvert | Chaîne d'actions par lot pour images | Images seulement | Modèle « recette » linéaire pour débutants |
| VERT (AGPL, Svelte, WebAssembly) | Interface minimaliste très soignée | Limité à ce que WASM sait faire | Sobriété visuelle, parcours en 3 étapes |
| ConvertX (AGPL, Bun, auto-hébergé) | 1000+ formats via ~20 convertisseurs | Interface basique, serveur | Inventaire des moteurs et formats |
| CloudConvert (SaaS) | Recherche « X vers Y », formulaires d'options par format | En ligne (confidentialité) | Schémas d'options par format |
| Czkawka / Krokiet (Rust) | Doublons, images/vidéos/musique similaires | Outil séparé | Algorithmes (hash partiels, pHash, BK-tree) |
| Format Factory, Any Video Converter… | Large couverture | Publicités, installeurs douteux, opacité | Contre-modèles : transparence totale |
| Permute (macOS) | Glisser-déposer, simplicité exemplaire | macOS, payant | Simplicité du premier contact |

## 5. Étude de `erkamyaman/rust-anything`

- **Nature** : outil CLI + serveur MCP + « skill » pour agents de code. Il **ne traduit pas** le
  code : il enregistre le comportement d'une application existante (TypeScript, Python, C, Go,
  Ruby, Java ; PHP/C# en préversion), puis vérifie que chaque morceau réécrit en Rust se comporte
  pareil (`record`, `diff`, `bench`, `shadow`, `gate`).
- **Licence** : MIT. **Maturité** : versions 1.0 à 1.3 publiées entre le 16 et le 18 novembre
  2026, ~126 commits, ~1,2 k étoiles — très jeune.
- **Utilité ici** : nulle pour l'écriture d'une application neuve (il faut un programme original
  à enregistrer). Utile **plus tard** si l'on veut porter en Rust un composant existant écrit en
  Python/TypeScript (par exemple un pipeline OCRmyPDF, un script fontTools ou un plugin
  communautaire), avec vérification comportementale. **Ce n'est pas une dépendance.**

## 6. Difficultés techniques identifiées

1. **Honnêteté de la fidélité** : « MKV → MP4 » est sans perte si les flux sont compatibles
   (remux), avec perte sinon. La fidélité dépend du **contenu réel** (codecs, canal alpha,
   profondeur de bits), pas seulement de l'extension → sonde du fichier avant planification.
2. **Chemins pour les outils externes** : Unicode, chemins longs Windows, noms commençant par
   `-`, noms interprétés comme protocoles par FFmpeg (`concat:`, `http:`) → chemins absolus,
   préfixe `file:` pour FFmpeg, mise en scène par lien physique avec un nom ASCII si nécessaire.
3. **Arbres de processus** : LibreOffice, Calibre ou Ghostscript lancent des sous-processus →
   Job Objects Windows / groupes de processus Unix pour une annulation fiable.
4. **LibreOffice** : instance unique par profil → un profil isolé par tâche, délais stricts.
5. **Fichiers piégés** : bombes de décompression (archives et images), traversée de chemins,
   liens symboliques, playlists HLS/concat FFmpeg capables de lire des fichiers locaux,
   délégués ImageMagick (ImageTragick), liens externes dans les documents bureautiques.
6. **Distribution des moteurs** : sources hétérogènes, sommes de contrôle pas toujours publiées
   (7-Zip), installeurs au lieu d'archives (Tesseract Windows, Calibre portable) → index signé
   maintenu par le projet (ADR-007).
7. **WebKitGTK** sous Linux : lecture vidéo et performances inférieures → aperçus générés côté
   backend.
8. **Licences** : AGPL (Ghostscript, MuPDF), GPL (x264/x265 dans FFmpeg GPL, Calibre, Pandoc),
   unRAR, brevets de codecs → aucun moteur embarqué dans l'installeur, téléchargement à la
   demande depuis la source officielle.

## 7. Fonctionnalités supplémentaires proposées

- Intégration au **menu contextuel** de l'Explorateur / Nautilus / Dolphin avec les profils favoris.
- **Palette de commandes** (Ctrl+K) : « convertir en webp », « ouvrir les moteurs »…
- **Mode simulation** (dry-run) pour lots et workflows : ce qui serait fait, sans rien écrire.
- **Nettoyage de confidentialité** : suppression GPS/EXIF/auteur avant partage.
- **Éditeur de métadonnées** (EXIF, tags audio, infos PDF, métadonnées ebook).
- **Comparateur A/B** image (rideau), vidéo (images synchronisées), audio (bascule).
- **Sommes de contrôle** : calcul et vérification de fichiers `.sha256`/`.md5`.
- **Renommage par modèles** avec jetons issus des métadonnées (`{exif.date}`, `{artist}`).
- **Planches contact** et miniatures vidéo.
- **Journal d'opérations réversibles** (déplacements, mises en corbeille) avec annulation.
- Moteurs IA **locaux et optionnels** (phase 4) : sous-titres par whisper.cpp, agrandissement
  Real-ESRGAN (ncnn-vulkan), détourage — jamais d'envoi de fichier sur un serveur.
- CLI `chud` complète avec sortie JSON pour les scripts.
