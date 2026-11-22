# Journal des décisions techniques (ADR)

Chaque décision indique son statut : **Acceptée**, **Proposée** (en attente de validation du
porteur du projet) ou **Remplacée**. Une décision n'est jamais effacée : on en ajoute une
nouvelle qui la remplace.

---

## ADR-001 — Cœur en Rust, organisé en workspace de crates
**Statut : Acceptée**

Le cœur (formats, détection, planification, exécution, moteurs, gestionnaire de moteurs,
tâches) est écrit en Rust et découpé en crates indépendantes de toute interface. La même
bibliothèque alimente l'application graphique et la CLI `chud`.

*Pourquoi* : sûreté mémoire face à des fichiers hostiles, performances, binaires autonomes,
excellent support Windows/Linux, et le souhait exprimé du porteur.

## ADR-002 — Interface : Tauri 2 + Svelte 5 + TypeScript
**Statut : Acceptée** (validée par le porteur le 2026-11-18)

Alternatives étudiées : Slint, Iced, egui (voir [RESEARCH.md §2](RESEARCH.md#2-technologie-dinterface)).

*Pourquoi* : seule option offrant à la fois une finition visuelle de niveau « produit », un
éditeur de nœuds mature (Svelte Flow), des aperçus image/vidéo/PDF, une accessibilité solide,
et un bundler + updater signés intégrés. Svelte plutôt que React : réactivité fine (runes) adaptée
aux mises à jour de progression à haute fréquence sur de longues listes, runtime minimal.

*Conséquences* : deux langages ; l'interface reste un **client léger** : aucune logique métier
dans le frontend. Les types Rust sont exportés vers TypeScript (tauri-specta) pour un contrat
typé de bout en bout. Les aperçus médias sont produits par le backend (cf. risque WebKitGTK).

## ADR-003 — Exécution synchrone sur pool de threads plutôt qu'async
**Statut : Acceptée**

Les conversions sont exécutées par un pool borné de threads ouvriers ; l'annulation passe par
un jeton partagé et la terminaison de l'arbre de processus. Les événements de progression
transitent par des canaux `crossbeam`.

*Pourquoi* : le travail est dominé par des processus externes et du calcul CPU ; l'async
n'apporte rien sinon de la complexité (traits non `dyn`-compatibles, blocages cachés). Les
téléchargements du gestionnaire de moteurs et l'hôte Tauri restent libres d'utiliser Tokio et
appellent le cœur via `spawn_blocking`.

## ADR-004 — Moteurs déclaratifs : capacités, fidélité, options
**Statut : Acceptée**

Chaque moteur implémente le trait `Engine` et **déclare** : conversions (formats lus → écrits),
fidélité de chaque arête (sans perte / avec perte / dépend du contenu), caractéristiques
conservées (alpha, EXIF, chapitres…), options (schéma typé rendu automatiquement par l'interface),
version, état. Les moteurs externes **sondent** leurs capacités réelles (ex. `ffmpeg -encoders`)
au lieu de les supposer.

## ADR-005 — Planification par graphe avec coût lexicographique
**Statut : Acceptée**

Les capacités forment un graphe orienté (nœuds = formats, arêtes = moteur × conversion).
Le chemin retenu minimise, dans l'ordre : (1) le nombre d'étapes avec perte, (2) les pertes de
caractéristiques, (3) le nombre d'étapes, (4) le coût de préférence du moteur. Longueur maximale
3 étapes. Une conversion directe avec perte n'est jamais remplacée par un détour plus long
« sans perte » qui ne le serait pas réellement.

## ADR-006 — Format catalogue en données (TOML) embarqué
**Statut : Acceptée**

Les formats (identifiant, catégorie, extensions, types MIME, compression, caractéristiques
supportées) sont décrits dans `crates/chud-core/data/formats.toml`. Ajouter un format = ajouter
une entrée, sans toucher au code. La détection combine signature binaire (crate `file-format`),
inspection de conteneur (ZIP → DOCX/EPUB/ODT…), heuristiques de contenu texte et, en dernier
recours, l'extension (avec confiance faible signalée).

## ADR-007 — Distribution des moteurs : index signé + vérification amont
**Statut : Proposée** (le format de l'index et la clé de signature sont à créer)

Hébergement retenu : le dépôt GitHub du projet, `BigFatDeuce/chud-converter` (index publié
avec les versions). La clé privée de signature sera créée et conservée par le porteur seul ;
seule la clé publique est intégrée à l'application.

L'application ne fait confiance qu'à :
1. un **index de moteurs signé** (minisign/Ed25519) publié par le projet, qui associe chaque
   version de moteur à ses URL officielles et à leurs empreintes SHA-256 (épinglées par la CI
   après vérification des sommes/signatures amont) ;
2. en l'absence d'index joignable, aux **sommes de contrôle publiées par l'amont**
   (ex. `checksums.sha256` de BtbN) — si l'amont n'en publie pas, l'installation est refusée
   sauf confirmation explicite « non vérifié ».

Installations isolées sous `<données>/engines/<id>/<version>/`, sans droits administrateur,
pointeur `current` → retour arrière instantané. Une installation gérée par le système (apt,
winget, Program Files…) n'est **jamais** modifiée : elle est détectée et utilisée en lecture seule.

## ADR-008 — Aucun moteur lourd embarqué dans l'installeur
**Statut : Acceptée**

L'installeur ne contient que l'application (quelques dizaines de Mo). Les moteurs sont
téléchargés **à la demande**, au moment où une conversion en a besoin, avec affichage de la
taille et de la licence. Cela réduit le premier téléchargement et évite d'avoir à redistribuer
des composants GPL/AGPL/brevetés.

## ADR-009 — Mise en scène (staging) et finalisation atomique
**Statut : Acceptée**

Chaque tâche travaille dans un répertoire temporaire dédié (`<cache>/staging/<job>`). Les
sorties sont vérifiées (existence, taille, re-détection du format, décodage par un outil
indépendant lorsque possible) **avant** d'être déplacées vers la destination finale. Le
fichier source n'est jamais modifié. Les conflits de noms sont résolus par réservation
(`create_new`) pour éviter toute course.

## ADR-010 — Exécution de processus durcie
**Statut : Acceptée**

Jamais de shell : programme absolu + vecteur d'arguments. Jamais de recherche dans le `PATH`
au moment de l'exécution (le chemin est résolu une fois par le gestionnaire de moteurs). Arbre
de processus confiné (Job Object Windows avec `KILL_ON_JOB_CLOSE` et limite mémoire, groupe de
processus + `setrlimit` sous Unix), délai maximal, annulation fiable, fenêtre console masquée.

## ADR-011 — Plugins : trois niveaux, sans code natif tiers
**Statut : Proposée**

1. **Adaptateurs déclaratifs** (TOML) : décrire un outil CLI (formats, modèles d'arguments,
   vérification) — pas de code exécuté hors de l'outil déclaré.
2. **Plugins WebAssembly** (Wasmtime + Component Model, interfaces WIT typées) : convertisseurs,
   analyseurs et optimiseurs purs, sans accès réseau ni système de fichiers hors des flux fournis,
   avec limites de mémoire et de temps (epoch interruption).
3. **Moteurs intégrés** (Rust, compilés dans l'application) : réservés au projet.

Les bibliothèques natives (`.dll`/`.so`) de tiers sont refusées. Signature minisign obligatoire
hors « mode développeur ». Extism a été écarté au profit du Component Model (contrats typés,
standard, maintenu par Bytecode Alliance).

## ADR-012 — Persistance : SQLite pour l'historique, TOML pour la configuration
**Statut : Acceptée**

Historique, empreintes de doublons, états des dossiers surveillés → SQLite (`rusqlite`,
SQLite embarqué). Préférences, profils et workflows → fichiers TOML/JSON lisibles et partageables.
Mode portable : présence d'un fichier `portable` à côté de l'exécutable → tout est stocké dans
`./data`.

## ADR-013 — Doublons : trois niveaux jamais confondus
**Statut : Acceptée**

1. **Identiques** (octet pour octet) : taille → hash partiel → BLAKE3 complet (→ comparaison
   octet par octet optionnelle).
2. **Même contenu** (ex. PNG et BMP aux pixels identiques, FLAC et WAV au PCM identique) :
   hash du contenu décodé.
3. **Similaires** (redimensionné, recompressé, réencodé) : empreintes perceptuelles (pHash/dHash,
   Chromaprint, séquences d'images clés) avec score de similarité affiché.

Suppression : corbeille du système par défaut, suppression définitive uniquement sur demande
explicite et double confirmation.

## ADR-014 — Licence de l'application
**Statut : Acceptée** (décision du porteur le 2026-11-18)

L'application est publiée sous **GPL-3.0-or-later** (fichier `LICENSE`). Conséquences : les
dérivés restent libres ; des bibliothèques GPL (pngquant/libimagequant…) peuvent être intégrées
en processus ; toute dépendance doit être compatible avec la GPL-3.0 (contrôle `cargo-deny`).
La licence du futur SDK de plugins sera décidée avec lui (recommandation : MIT/Apache-2.0, pour
que des plugins de toute licence puissent exister).

Alternative écartée : MIT/Apache-2.0 pour tout (aurait permis une version commerciale fermée).

## ADR-015 — Mises à jour de l'application ≠ mises à jour des moteurs
**Statut : Acceptée**

L'application se met à jour via l'updater Tauri (paquets signés Ed25519, canal stable/bêta).
Les moteurs se mettent à jour via le gestionnaire de moteurs (ADR-007), indépendamment,
avec épinglage de version et retour arrière.

## ADR-016 — `rust-anything` n'est pas une dépendance
**Statut : Acceptée**

Outil d'agent pour porter vers Rust du code existant en vérifiant son comportement enregistré.
Sans intérêt pour du code neuf ; éventuellement utile plus tard pour porter un composant Python
ou TypeScript (voir RESEARCH §5).

## ADR-017 — Intégration au menu contextuel
**Statut : Acceptée** (conception ; choix « sans certificat » du porteur le 2026-11-18)

Demande du porteur : une intégration au clic droit comparable à *File Converter*.
Décision : gestionnaire `IExplorerCommand` écrit en Rust (DLL COM minimale, inscrite par
utilisateur dans HKCU), qui **ne convertit rien lui-même** et transmet toute la sélection, en un
seul appel, à `chud-gui` (instance unique, fenêtre de progression compacte). Préréglages
modifiables partagés avec l'application. Linux : service menus Dolphin, extension ou scripts
Nautilus, actions Nemo et Thunar. Repli universel : « Envoyer vers ».

Alternatives écartées : verbes statiques du registre (un processus et une fenêtre console par
fichier, limite de 15 fichiers, sous-menus figés) ; extension COM .NET (SharpShell, utilisée par
File Converter : charge le CLR dans l'Explorateur).

Signature : **pas de certificat de signature de code pour l'instant**. Sous Windows 11, la
commande apparaît donc dans le menu classique (« Afficher plus d'options », Maj+F10), comme
File Converter. Le paquet MSIX « sparse » signé, qui donnerait accès au menu court, sera
reconsidéré lors d'une diffusion à grande échelle. Détails : `docs/SHELL-INTEGRATION.md`.

## ADR-019 — Menu contextuel : verbes statiques d'abord, DLL ensuite
**Statut : Acceptée** (2026-11-18 ; complète ADR-017)

Première version du menu Windows : des **clés de registre** de l'utilisateur
(`SystemFileAssociations\.<ext>\shell\ChudConverter` avec `ExtendedSubCommandsKey`), et non
la DLL `IExplorerCommand` d'ADR-017. Les raisons qui avaient écarté les verbes statiques ont été
levées une à une et mesurées :

- *une fenêtre console par fichier* : `chud-converter` est une application graphique, il n'y a
  pas de console ;
- *limite de 15 fichiers* : `MultiSelectModel=Player` la supprime ;
- *un processus par fichier* : chacun transmet ses arguments à la première instance et se termine
  (60 lancements simultanés : relais terminés en moins d'une seconde, 60/60 convertis) ;
- *sous-menus figés* : ils sont réécrits à chaque modification des préréglages.

Avantage décisif : rien n'est chargé dans le processus de l'Explorateur (une DLL défaillante peut
le faire planter), et l'inscription comme le retrait sont de simples clés, listées et vérifiées.
Limite assumée : le sous-menu dépend de l'**extension** du fichier, pas de son contenu réel ;
le contenu est vérifié au moment de convertir, et un fichier qui ne convient pas est signalé
dans la fenêtre de conversion. La DLL reste prévue pour un sous-menu selon le contenu et la
transmission de la sélection en un seul appel.

## ADR-018 — Nom du produit : Chud Converter
**Statut : Acceptée** (décision du porteur le 2026-11-18)

Le produit s'appelle **Chud Converter** partout : fenêtres, installeur, métadonnées de
l'exécutable, dossier de données (`%LOCALAPPDATA%\Chud Converter`, `~/.local/share/chudconverter`
selon les conventions de chaque système), dépôt `chud-converter`. La commande en ligne reste
`chud` (courte à taper). Le nom « Chud Convert » du pack d'icônes n'est plus utilisé.

## ADR-020 — Deux langues, textes en ligne

**Décision.** Le français et l'anglais sont écrits côte à côte, à l'endroit où le texte est
produit : `tr!("…", "…")` et `trf!("… {x}", "… {x}")` dans le code Rust, `L("…", "…")` dans
l'interface ; `name_en` / `limitations_en` dans `formats.toml`, `description_en` /
`license_en` dans les manifestes de moteurs. Le choix (`system`, `fr`, `en`) est enregistré
dans le fichier de réglages partagé (`engines.toml`, clé `language`) : l'application, la ligne
de commande et le menu clic droit parlent la même langue. Par défaut, langue du système
(français si le système est en français, anglais sinon).

**Pourquoi.** Deux langues seulement : des fichiers de traduction par clés (Fluent, gettext)
ajouteraient une indirection sans bénéfice, et les clés oubliées passent inaperçues. Écrits
ensemble, les deux textes ne divergent pas, le compilateur vérifie que les arguments des
messages formatés sont les mêmes, et un test refuse tout texte français resté dans le
catalogue anglais. Si une troisième langue arrive, ces paires se convertissent
mécaniquement en catalogue.

**Conséquences.** Changer de langue ne relance rien : l'interface est reconstruite et les
textes produits par le cœur (formats, cibles, rapports, préréglages, moteurs) sont redemandés.
Les rapports déjà écrits restent dans leur langue. Les préréglages fournis suivent la langue ;
un nom choisi par l'utilisateur est conservé tel quel. Un fichier de réglages illisible n'est
jamais écrasé : le changement est refusé et l'erreur affichée.

## ADR-022 — Installer les moteurs depuis l'application

**Contexte.** Demander d'ouvrir un terminal et de taper `winget install …` décourage la plupart
des utilisateurs (2026-11-18 : « il faut que ce soit simple à utiliser », puis « tout doit se
faire depuis notre logiciel »).

**Décision.** FFmpeg et 7-Zip s'installent d'un clic dans l'écran Moteurs (et par
`chud engines install`). La source de chaque paquet (adresse HTTPS, taille, SHA-256) est figée
dans le manifeste du moteur, **intégré au programme signé** : une nouvelle version d'un moteur
arrive avec une mise à jour de l'application, vérifiée par sa signature (ADR-021). L'index signé
indépendant (ADR-007) reste prévu pour suivre les moteurs entre deux versions de l'application.

- Sources : FFmpeg = compilation « essentials » de gyan.dev (citée par ffmpeg.org) sur GitHub ;
  7-Zip = publication officielle d'Igor Pavlov sur GitHub. Empreintes relevées sur les
  publications (champ `digest` des fichiers GitHub).
- Avant tout téléchargement, l'interface montre la version, la taille, l'éditeur, la licence et
  un lien vers la publication ; l'utilisateur confirme.
- L'installeur 7-Zip pour Windows n'est jamais exécuté : il est ouvert comme une archive par
  `7zr.exe`, lui-même vérifié par son empreinte.
- Installé dans `<données>/engines/<id>` ; les installations du système ne sont jamais touchées.
  Retrait et retour arrière en un clic (avec confirmation).
- Moteur déjà prêt depuis le système : une copie gérée peut être posée **à côté**, seulement sur
  demande explicite (bouton discret, confirmation qui le dit ; `--alongside` en ligne de
  commande). Elle est alors utilisée en priorité ; la retirer rend la main à celle du système.
  Un chemin choisi par l'utilisateur passe avant tout : aucune copie n'est alors proposée.
  Règle unique pour l'application et la CLI : `install::check_allowed`.
- **Aucune commande à taper ou à copier dans l'application** (2026-11-18 : « je ne veux pas que
  l'utilisateur ait à copier/coller un lien dans son terminal »). Les commandes des
  gestionnaires de paquets ne restent que dans `chud engines`, pour qui utilise déjà la ligne
  de commande.
- FFmpeg pour Linux (x86_64, ARM64) : compilation GPL de BtbN (citée par ffmpeg.org), prise
  dans une publication de **fin de mois** (`autobuild-AAAA-MM-JJ-…`), conservée environ deux
  ans, et non dans « latest » qui change chaque jour. johnvansickle.com écarté : adresse
  mouvante, version ancienne. L'archive .tar.xz (≈ 540 Mo décompressée) est extraite en flux
  vers le disque, avec progression, plafond de 2 Gio, liens et chemins sortants refusés.
- Les moteurs connus que Chud Converter ne sait pas encore utiliser sont rangés à part,
  « Bientôt pris en charge », sans bouton d'installation : rien ne s'en servirait encore
  (depuis la 0.2.0, ADR-023, il n'y en a plus).
- Premier démarrage (2026-11-18 : « mets un message au premier démarrage pour indiquer qu'il
  faut les télécharger, […] 1 par 1 ou tous d'un seul coup ») : s'il manque des moteurs, un
  accueil les présente (usage, taille, version, source, licence) avec un bouton par moteur et
  « Tout installer » ; montré une fois (marque `intro-seen` dans le dossier de configuration,
  à part des réglages, qui refusent les champs inconnus des versions précédentes). Ensuite, un
  bandeau dans Convertir propose les moteurs manquants, tous ou un seul.
- Un fichier sans conversion possible propose directement le moteur qui manque (« installer
  FFmpeg (110 Mo) ») ; la confirmation s'ouvre dans l'écran Moteurs, puis l'application revient
  à la conversion, le fichier toujours sélectionné.
- Le flux `.github/workflows/engine-sources.yml` installe réellement chaque moteur sur chaque
  plateforme proposée (Windows, Linux x86_64, Linux ARM64) chaque semaine et à chaque
  changement de source : une adresse retirée ou une empreinte modifiée est repérée avant les
  utilisateurs.

## ADR-023 — Les onze autres moteurs : adaptateurs et installation

**Contexte.** 2026-11-19 : « fais TOUT » — Pandoc, LibreOffice, Calibre, ImageMagick, libvips,
qpdf, Ghostscript, Tesseract, ExifTool, MediaInfo et MKVToolNix doivent être réellement utilisés
et s'installer depuis l'application, sans terminal à ouvrir ni commande à copier.

**Décision.**

- **Un adaptateur par moteur** (`crates/chud-engines/src/<id>.rs`), sur un socle commun
  (`tool.rs`) : chaque conversion travaille dans un dossier de travail unique où l'entrée est
  liée ou copiée sous un nom simple (`input.ext`). Aucun nom de fichier de l'utilisateur n'est
  donc jamais lu par un outil comme un motif (`[0]` d'ImageMagick, `%d` de Ghostscript, `:`
  des préfixes de format). Arguments séparés, jamais de ligne de shell.
- **Vérification propre à chaque moteur**, jamais « le processus a rendu 0 » : relecture par
  Pandoc en texte brut, contrôle des archives OOXML/ODF membre par membre, en-tête MOBI,
  `qpdf --check` et nombre de pages, rendu Ghostscript sans sortie, décodage de l'image,
  `vipsheader`/`vips avg`, `mkvmerge -J` avec comparaison du nombre de pistes. Un OCR sans texte
  reconnu est un échec, pas un fichier vide « réussi ».
- **MediaInfo et ExifTool ne convertissent rien** : ils enrichissent l'analyse du fichier
  (`Engine::enriches_probe`). Le moteur principal analyse d'abord, puis leurs lignes et leurs
  caractéristiques (HDR, EXIF, XMP, IPTC, ICC) s'ajoutent ; s'il échoue, il n'y a pas d'analyse.
- **Coûts** : à fidélité égale, le moteur spécialisé gagne (qpdf pour PDF→PDF, libvips puis
  ImageMagick pour les images, LibreOffice avant Pandoc pour la bureautique, Tesseract en
  dernier car la reconnaissance est incertaine).
- **Installation Windows sans droits administrateur**, depuis les publications officielles,
  empreintes figées dans les manifestes :
  - zip (Pandoc, qpdf, libvips, ExifTool, MediaInfo, MKVToolNix) ;
  - archive 7z ouverte par `7zr` (ImageMagick) ;
  - installeur NSIS **jamais exécuté**, ouvert comme une archive par 7-Zip (Ghostscript,
    Tesseract ; la langue française de Tesseract est ajoutée, vérifiée par son empreinte) ;
  - paquet MSI déplié en installation administrative (`msiexec /a`, sans rien inscrire dans le
    système) : LibreOffice (≈ 380 Mo, son runtime Visual C++ est recopié depuis le paquet) et
    Calibre (≈ 290 Mo).
  - ImageMagick et Ghostscript ont besoin du runtime Visual C++ de Microsoft : s'il manque, la
    confirmation le dit, puis le paquet officiel (adresse et empreinte figées) est installé ;
    Windows demande alors l'autorisation (UAC). Refus : l'installation s'arrête proprement.
- **Linux** : version portable officielle quand elle existe (Pandoc, qpdf, ImageMagick,
  Calibre, FFmpeg, 7-Zip). Sinon, **paquets de la distribution** (apt, dnf, pacman, zypper,
  noms validés) : l'application lance elle-même `pkexec`, qui affiche la demande de mot de passe
  du système ; sans pkexec, un terminal s'ouvre tout seul avec la commande déjà lancée (rien à
  copier). La confirmation dit que le gestionnaire de paquets du système est utilisé. Un paquet
  du système reste au système : l'application ne le retire ni ne le met à jour.
- Plafonds d'extraction relevés à 4 Gio et 80 000 entrées (LibreOffice) ; liens d'archives
  recopiés seulement s'ils restent dans l'archive.

**Conséquences.** Tous les moteurs connus sont utilisables : la rubrique « Bientôt pris en
charge » disparaît. La CI Linux installe tous les outils et lance les tests réels des
adaptateurs (`CHUD_REQUIRE_TOOLS`) ; le flux hebdomadaire installe chaque source gérée.

## ADR-021 — Publication et mise à jour de l'application
**Statut : Acceptée** (précise ADR-015)

**Décision.**
- Publication par tag `vX.Y.Z` (`.github/workflows/release.yml`) : brouillon, paquets construits
  par la CI, `latest.json`, `SHA256SUMS.txt`, puis publication seulement si tout a réussi.
- Windows : installeur NSIS **par utilisateur** (comme le menu clic droit, qui est dans HKCU) et
  ZIP portable. Pas de MSI : il s'installe pour la machine, alors que le menu est par compte, et
  sa désinstallation ne pourrait pas retirer proprement le menu de chaque utilisateur.
- Linux : AppImage, .deb, .rpm.
- Mise à jour : extension officielle de Tauri. Paquets signés Ed25519, version signée exigée
  (`requireSignedVersion`) : un `latest.json` falsifié ne peut ni changer de fichier ni faire
  passer une ancienne version pour une nouvelle.
- Recherche automatique au démarrage, au plus une fois par jour, désactivable dans Réglages
  (choix de l'utilisateur, 2026-11-18). Seul `latest.json` est lu ; aucun fichier ni nom de fichier
  n'est envoyé. Installation seulement sur clic, jamais pendant une conversion.
- Version portable, .deb/.rpm et version de développement : la nouvelle version est signalée
  avec un lien, pas installée par le programme (il ne remplace pas ce qu'un autre outil gère).
- Désinstallation Windows : `chud-converter --uninstall-menu` retire le menu s'il lance ce
  programme-ci ; pas lors d'une mise à jour (`/UPDATE`), où le chemin ne change pas.

**Conséquences.** La clé privée de signature est un secret GitHub (`TAURI_SIGNING_PRIVATE_KEY`)
et une copie chez le mainteneur ; sa perte oblige les utilisateurs à réinstaller à la main.
Sans certificat de signature de code, SmartScreen avertit au premier lancement.

## ADR-024 — Opérations : fusionner, diviser, pivoter, archiver
**Statut : Acceptée**

**Contexte.** Une conversion va d'un fichier vers un format. Fusionner des PDF ou réunir des
fichiers dans une archive n'entre pas dans ce modèle (plusieurs sources, ordre, plusieurs
résultats), et l'y forcer aurait faussé le planificateur.

**Décision.**
- Un **catalogue fixe d'opérations** dans le cœur (`chud_core::operations()`), séparé du graphe des
  conversions : l'interface montre un outil même si son moteur manque, et propose de l'installer.
- Chaque moteur déclare les opérations qu'il exécute (`Engine::operations`), annonce ce qui se perd
  pour ces fichiers précis (`assess_operation`), exécute, puis vérifie (`verify_operation`).
- `chud-jobs` les exécute comme une conversion : dossier de travail, vérification, contrôle du
  format produit, finalisation atomique, jamais par-dessus une des sources.
- **qpdf** pour les PDF, pilotable sans rien recompresser. Les parties d'une division repartent
  d'un document vide (`--empty`) : un extrait fait à partir de l'original garde, cachés, les titres
  des signets et les balises des autres pages. Les signets utiles sont ensuite réécrits et relus
  (`--update-from-json`, qpdf 11 ou plus récent ; avant, l'outil le dit et continue sans signets).
- **Perdu et annoncé** : balises d'accessibilité, pièces jointes, liens internes nommés, signatures
  des fichiers fusionnés après le premier (et de toutes les parties d'une division).
- **PDF protégés** : refusés pour fusionner et diviser (la protection disparaîtrait) ; pivoter garde
  la protection telle quelle.
- **7-Zip** pour les archives : chemins absolus dans un fichier liste (seul le nom de l'élément est
  enregistré, vérifié sous Windows et Linux), homonymes renommés « nom (2) », liens refusés.

**Conséquences.** Une nouvelle opération (filigrane, découpe vidéo, assemblage d'images en PDF…)
s'ajoute au catalogue et à un moteur, sans toucher au planificateur ni à l'écran Outils.
