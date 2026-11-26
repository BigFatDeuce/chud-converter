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
- [x] Adaptateurs Pandoc, LibreOffice, Calibre, ImageMagick, libvips, qpdf, Ghostscript, Tesseract, MKVToolNix ; analyse enrichie par MediaInfo et ExifTool ; chacun vérifie son résultat à sa façon (ADR-023) — 0.2.0
- [x] Détection des moteurs installés (gérés, emplacements connus, PATH) et sonde de capacités
- [x] Opérations (plusieurs fichiers vers un, un vers plusieurs) : fusionner, diviser, pivoter des PDF, créer une archive ; `chud tool` ; 8 tests réels avec noms piégés, relus par un chemin indépendant (ADR-024)
- [x] CLI `chud` : `formats`, `engines`, `detect`, `targets`, `plan`, `convert`, `matrix`, sortie JSON, motifs `*.ext` sous Windows
- [x] Tests unitaires et d'intégration sur fichiers réels générés (158 tests Rust, dont 8 tests réels des onze nouveaux adaptateurs avec des noms piégés `é [1] %d` ; dont une matrice permanente de 840 conversions réelles vérifiées, et depuis la 0.3.0 une matrice « documents » : chaque document, tableur, présentation, PDF, livre et image avancée converti vers chacune de ses cibles, puis chaque format obtenu relu à son tour (environ 1 800 conversions sous Windows et sous Linux) ; conflits, annulation, fichier tronqué, attaques d'archives, file d'attente, application, préréglages, menu clic droit, langues)
- [ ] Lecture testée automatiquement pour ARJ, LZH, RAR/CBR, ISO, DMG, VHD, SquashFS, RPM et LZMA : confiée à 7-Zip (test d'intégrité et contrôle de l'index à chaque conversion), mais aucun exemple n'est encore générable localement pour la matrice
- [x] Qualité : `clippy` sans avertissement, `rustfmt`, documentation utilisateur (README, BUILDING)

### 2b. Gestionnaire de moteurs complet
- [x] Téléchargement en flux + SHA-256 + extraction contrôlée + installation versionnée (FFmpeg et 7-Zip : Windows x64, Linux x64 et ARM64 ; installation réelle vérifiée chaque semaine en CI) — 0.1.2
- [x] Les treize moteurs installables depuis l'application, sans droits administrateur sous Windows : zip, 7z, tar.gz/tar.xz, installeurs NSIS ouverts comme archives, MSI dépliés (`msiexec /a`), fichiers de données ajoutés (langue française de Tesseract), runtime Visual C++ officiel si besoin ; chemins trop longs pour Windows refusés clairement — 0.2.0 (installation réelle des 13 essayée sous Windows)
- [x] Linux sans version portable : paquets de la distribution (apt, dnf, pacman, zypper) par `pkexec`, ou un terminal ouvert avec la commande déjà lancée — 0.2.0 (code et tests unitaires)
- [x] Linux : installation réelle des treize moteurs par l'application sous Fedora 45 (dnf par `pkexec`), AppImage comprise ; LibreOffice installé sans Calc ni Impress reconnu comme incomplet, puis complété en un clic — 0.3.0
- [ ] Linux : installation par `pkexec` essayée sur un vrai bureau (GNOME, KDE), avec la fenêtre de mot de passe
- [x] Désinstallation (essayée en vrai), retour à la version précédente (testé sur installations simulées : une seule version publiée par moteur pour l'instant) — 0.1.2
- [ ] Épinglage, canaux
- [ ] Index signé (format, outil de génération en CI, clé)
- [x] Manifestes de détection : FFmpeg, 7-Zip, Pandoc, LibreOffice, Calibre, ImageMagick, libvips, qpdf, Ghostscript, Tesseract, ExifTool, MediaInfo, MKVToolNix
- [x] Sources de téléchargement vérifiées dans les manifestes pour les treize moteurs — 0.2.0
- [ ] Typst, DuckDB
- [x] Adaptateurs pour tous les moteurs détectés — 0.2.0

### 2c. Interface (Tauri 2 + Svelte 5, ADR-002)
- [x] Application de bureau : glisser-déposer, ajout de fichiers ou de dossiers, fichiers passés au lancement, format réel et extension trompeuse affichés
- [x] Sélecteur de format : recherche, catégories, formats communs à plusieurs fichiers (fidélité la moins bonne retenue), pastilles de fidélité
- [x] Plan et rapport de fidélité avant conversion, recalculés quand les options changent
- [x] Panneau d'options généré depuis les `OptionSpec`, options avancées repliées
- [x] File d'attente : ajout pendant les conversions, progression et vitesse par tâche, annulation (une ou toutes), rapport détaillé avec niveau de vérification réellement atteint, « Afficher dans le dossier »
- [x] Destination (à côté de l'original ou dossier), conflits de noms, date d'origine
- [x] Écran Moteurs (état, version, origine, licence) et écran À propos ; aucune commande à copier (0.1.2)
- [x] Testé sous Windows sur de vrais fichiers (pilotage par UI Automation) ; compilation Linux en CI
- [ ] Liste virtualisée (milliers de fichiers), progression globale, ETA, relance d'une tâche échouée
- [x] Écran Outils : choix de l'outil, ordre des fichiers, aperçu de ce qui se perd, fichiers laissés de côté, moteur manquant installable ; accès depuis Convertir — 0.5.0
- [ ] Favoris et formats récents
- [x] Installation des moteurs à la demande depuis l'écran Moteurs, ou depuis un fichier qui en a besoin — 0.1.2
- [ ] Préférences complètes (modèle de nom, métadonnées, parallélisme, cache, thème forcé, portable)
- [x] Français et anglais partout (application, rapports, erreurs, ligne de commande, menu clic droit), choix dans **Réglages**, `chud language`, `--lang` ; langue du système par défaut
- [ ] Contrôle complet au clavier, tests d'interface automatisés

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
- [x] PDF : fusion, division, rotation, extraction de pages, compression (qpdf) ; OCR Tesseract
- [ ] PDF : filigrane, PDF/A, formulaires et mots de passe
- [ ] Livres électroniques : Calibre, CBZ/CBR natifs
- [x] Archives : réunir plusieurs fichiers et dossiers dans une archive (ZIP, 7z, TAR, TGZ, TBZ2, TXZ)
- [ ] Archives : extraction native sécurisée (zip/tar/gz/xz/bz2/zst/7z), multipartie, test d'intégrité
- [ ] Données : XML, XLSX/ODS (calamine), Parquet, SQLite, détection et conversion d'encodages

### 2e. Distribution
- [x] Windows : installeur NSIS par utilisateur (sans droits d'administrateur, anglais et français, retire le menu clic droit à la désinstallation), version portable ZIP ; MSI écarté (ADR-021)
- [x] Linux : AppImage, .deb, .rpm (construits par la CI ; **pas encore essayés sur un vrai bureau Linux**)
- [x] Publication par tag : brouillon, paquets signés, `latest.json`, `SHA256SUMS.txt`, publication seulement si tout a réussi
- [x] Mise à jour intégrée (updater Tauri, signature Ed25519 et version signée exigées) : recherche au démarrage au plus une fois par jour, désactivable ; installation sur clic ; refusée pendant une conversion ; testée sous Windows (0.0.9 → 0.1.0, paquet falsifié refusé)
- [ ] Canal bêta ; signature de code Windows (certificat)
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
- [x] Relecture indépendante du code : planificateur comparé à une recherche exhaustive, fin des processus bornée, sortie des moteurs bornée, dossiers résultats placés d'un coup — 0.5.1
- [x] Vérification du texte des documents bureautiques : les mots de l'original retrouvés dans le résultat (LibreOffice, Calibre)
- [ ] Vérification des tableaux, images et formules des documents (aujourd'hui : structure seulement)
- [x] Linux : descendants sortis du groupe de processus (`setsid`) arrêtés aussi ; Windows : descendants confinés vérifiés (démarrage suspendu écarté : 0,1 à 0,2 s de plus par lancement pour une fenêtre de quelques microsecondes)
- [x] Nettoyage : jamais les fichiers d'un autre programme, ni les dossiers de travail d'une autre instance en cours
- [ ] 3D, polices, sous-titres avancés, géo, scientifique, formats anciens
- [ ] Moteurs IA locaux optionnels (sous-titres whisper.cpp, agrandissement, détourage)
- [ ] macOS
