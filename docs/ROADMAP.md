# Feuille de route

Règle : une fonctionnalité n'est cochée que lorsqu'elle est **implémentée et testée** sur des
fichiers réels. Ce qui est seulement déclaré ou prévu reste décoché.

## Phase 1 — Recherche et conception ✅
- [x] Étude des technologies d'interface, des moteurs, des licences, des concurrents, de rust-anything
- [x] Architecture, décisions (ADR), sécurité, gestionnaire de moteurs, plugins, workflows
- [ ] Validation par le porteur des décisions « Proposées » (ADR-002, 007, 011, 014, 017)

## Phase 2 — Socle (en cours)
### 2a. Cœur (fait dans cette itération)
- [x] Workspace Rust, catalogue de formats en données, détection (signature, conteneur, texte, extension)
- [x] Trait `Engine`, capacités, fidélité, schéma d'options, registre
- [x] Planificateur par graphe avec coût lexicographique et rapport de fidélité honnête
- [x] Exécution de processus durcie (confinement, délais, annulation)
- [x] Tâches : staging, vérification, finalisation atomique, conflits de noms, pool, événements
- [x] Moteurs : image natif, données natif (JSON/YAML/TOML/CSV/TSV/NDJSON), FFmpeg, 7-Zip
- [x] Détection des moteurs installés (gérés, emplacements connus, PATH) et sonde de capacités
- [x] CLI `chud` : `formats`, `engines`, `detect`, `targets`, `plan`, `convert`, `matrix`, sortie JSON, motifs `*.ext` sous Windows
- [x] Tests unitaires et d'intégration sur fichiers réels générés (97 tests ; conflits, annulation, fichier tronqué, attaques d'archives, file d'attente, application, préréglages, menu clic droit)
- [x] Qualité : `clippy` sans avertissement, `rustfmt`, documentation utilisateur (README, BUILDING)

### 2b. Gestionnaire de moteurs complet
- [ ] Téléchargement en flux + SHA-256 + extraction contrôlée + installation versionnée
- [ ] Désinstallation, retour arrière, épinglage, canaux
- [ ] Index signé (format, outil de génération en CI, clé)
- [x] Manifestes de détection : FFmpeg, 7-Zip, Pandoc, LibreOffice, Calibre, ImageMagick, libvips, qpdf, Ghostscript, Tesseract, ExifTool, MediaInfo, MKVToolNix
- [ ] Sources de téléchargement vérifiées dans les manifestes (+ Typst, DuckDB)
- [ ] Adaptateurs pour les moteurs seulement détectés (MKVToolNix, Pandoc, LibreOffice, qpdf…)

### 2c. Interface (Tauri 2 + Svelte 5, ADR-002)
- [x] Application de bureau : glisser-déposer, ajout de fichiers ou de dossiers, fichiers passés au lancement, format réel et extension trompeuse affichés
- [x] Sélecteur de format : recherche, catégories, formats communs à plusieurs fichiers (fidélité la moins bonne retenue), pastilles de fidélité
- [x] Plan et rapport de fidélité avant conversion, recalculés quand les options changent
- [x] Panneau d'options généré depuis les `OptionSpec`, options avancées repliées
- [x] File d'attente : ajout pendant les conversions, progression et vitesse par tâche, annulation (une ou toutes), rapport détaillé avec niveau de vérification réellement atteint, « Afficher dans le dossier »
- [x] Destination (à côté de l'original ou dossier), conflits de noms, date d'origine
- [x] Écran Moteurs (état, version, origine, licence, commande d'installation officielle) et écran À propos
- [x] Testé sous Windows sur de vrais fichiers (pilotage par UI Automation) ; compilation Linux en CI
- [ ] Liste virtualisée (milliers de fichiers), progression globale, ETA, relance d'une tâche échouée
- [ ] Favoris et formats récents
- [ ] Installation des moteurs à la demande depuis l'écran Moteurs
- [ ] Préférences complètes (modèle de nom, métadonnées, parallélisme, cache, thème forcé, portable)
- [ ] Internationalisation FR/EN, contrôle complet au clavier, tests d'interface automatisés

### 2c bis. Menu contextuel (demandé par le porteur — avancé depuis la phase 3)
- [x] Conception (ADR-017, `docs/SHELL-INTEGRATION.md`)
- [x] Préréglages partagés CLI / application / menu (`presets.toml`, 21 fournis, chacun testé sur un vrai fichier) : `chud convert --preset`, `chud presets`, écran « Préréglages », application d'un préréglage dans l'écran Convertir, « Enregistrer comme préréglage »
- [x] `chud-converter --preset <id> -- fichiers…` : fenêtre de progression compacte, erreurs affichées, fermeture automatique seulement si tout a réussi
- [x] Instance unique : les lancements suivants rejoignent la première fenêtre (testé : 60 lancements simultanés, à froid et à chaud, 60/60 convertis)
- [x] Windows : menu en cascade par clés HKCU (sans droits d'administrateur, ADR-019), limité aux conversions possibles avec les moteurs présents, mis à jour quand les préréglages changent ; `chud shell install|uninstall|status` et boutons dans l'application ; testé dans le vrai menu de l'Explorateur (IContextMenu) puis retiré sans reste
- [ ] DLL `IExplorerCommand` (sous-menu selon le contenu réel, sélection transmise en un seul appel)
- [ ] Menu court de Windows 11 (paquet sparse signé, selon certificat)
- [x] Linux : menus de service Dolphin, actions Nemo, scripts Nautilus (code et tests unitaires ; **pas encore essayé sur un vrai bureau Linux**)
- [ ] Linux : Thunar (fichier `uca.xml` partagé, à modifier avec précaution)

### 2d. Couverture des catégories prioritaires
- [ ] Images : libvips, ImageMagick, encodeurs de référence (AVIF, JXL, HEIF), RAW, ICC/CMJN, conservation EXIF
- [ ] Audio : conservation des tags (`lofty`), normalisation EBU R128, découpe/fusion
- [ ] Vidéo : sélection de pistes, accélération matérielle (sonde réelle), sous-titres, découpe/assemblage
- [ ] Documents : LibreOffice, Pandoc, Typst
- [ ] PDF : qpdf, pdfium, lopdf (fusion, division, rotation, extraction, filigrane, compression, PDF/A), OCR Tesseract
- [ ] Livres électroniques : Calibre, CBZ/CBR natifs
- [ ] Archives : compresser un ou plusieurs fichiers (pas seulement un dossier), extraction native sécurisée (zip/tar/gz/xz/bz2/zst/7z), multipartie, test d'intégrité
- [ ] Données : XML, XLSX/ODS (calamine), Parquet, SQLite, détection et conversion d'encodages

### 2e. Distribution
- [ ] Windows : installeur NSIS (par utilisateur), MSI, version portable ZIP
- [ ] Linux : AppImage, .deb, .rpm
- [ ] Mise à jour de l'application (updater Tauri signé), canal stable/bêta
- [x] CI GitHub Actions Windows + Linux avec FFmpeg et 7-Zip réels (`fmt`, `clippy -D warnings`, tests)
- [ ] `cargo-deny`, `cargo-about` en CI

## Phase 3 — Fonctionnalités avancées
- [ ] Inspecteur universel (image, audio, vidéo, document, archive, données, police, 3D) + détection d'anomalies + aperçus
- [ ] Détecteur de doublons (identiques / même contenu / similaires), rapport, corbeille
- [ ] Optimisation automatique (objectifs, seuil de qualité SSIMULACRA2/VMAF, comparaison, aperçu avant/après)
- [ ] Profils et règles par type pour les lots
- [ ] Historique SQLite + relance
- [ ] Surveillance de dossiers
- [ ] Workflows (modèle DAG, éditeur visuel Svelte Flow, mode recette linéaire)
- [ ] Réparation de fichiers (sur copie, résultat honnête)
- [ ] Plugins : adaptateurs déclaratifs, puis WebAssembly, signatures, catalogue optionnel

## Phase 4 — Finition et extensions
- [ ] Performance (gros fichiers, milliers de fichiers), fuzzing, tests d'interface
- [ ] 3D, polices, sous-titres avancés, géo, scientifique, formats anciens
- [ ] Moteurs IA locaux optionnels (sous-titres whisper.cpp, agrandissement, détourage)
- [ ] macOS
