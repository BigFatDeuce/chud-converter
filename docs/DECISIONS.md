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
