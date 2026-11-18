# Intégration au menu contextuel (clic droit)

Objectif : convertir sans ouvrir l'application, comme le logiciel *File Converter* :
clic droit sur un ou plusieurs fichiers → **Chud Converter** → **En MP4** → une petite fenêtre de
progression, le résultat apparaît à côté de la source.

## 1. Expérience visée

```
Clic droit sur 12 fichiers .mov
└─ Chud Converter                      (icône de l'application)
   ├─ MP4 (compatible)               ← préréglages de la catégorie détectée
   ├─ MP4 (léger, 720p)
   ├─ WebM
   ├─ MP3 (extraire l'audio)
   ├─ GIF animé
   ├─ ─────────────
   ├─ Autre format…                  ← ouvre l'application, fichiers présélectionnés
   └─ Inspecter                      ← inspecteur universel (phase 3)
```

- Le sous-menu dépend du **type de la sélection** (vidéo, image, audio, document, archive, données…).
  Sélection mixte : seuls les préréglages communs, plus « Autre format… ».
- **Préréglages** modifiables dans l'application (nom, format, options, dossier de sortie,
  modèle de nom), comme les « presets » de File Converter, réordonnables, activables par type.
- Fenêtre de progression compacte : une ligne par fichier, annulation, rapport de fidélité
  accessible, notification système à la fin. Fermeture automatique si tout a réussi (réglable).
- Mêmes garanties que l'application : source jamais modifiée, conflits de noms → « (1) »,
  vérification de chaque résultat, erreurs détaillées.
- Dossiers : « Compresser en ZIP / 7z », « Convertir toutes les images du dossier… ».
- Archives : « Extraire ici », « Extraire dans un dossier », « Convertir en 7z / ZIP ».

## 2. Windows : architecture recommandée

### 2.1 Gestionnaire de commande `IExplorerCommand` (DLL COM en Rust)

Crate `chud-shellext` (bibliothèque `cdylib`, crate `windows` avec `#[implement]`) :

- implémente `IExplorerCommand` (titre, icône, état) et `IEnumExplorerCommand` (sous-menu dynamique) ;
- `GetState` / `EnumSubCommands` : lit la sélection (`IShellItemArray`), déduit les catégories
  **par extension seulement** (aucune lecture de fichier sur le fil de l'Explorateur, réponse
  en quelques millisecondes), puis filtre les préréglages ;
- `Invoke` : écrit la liste des fichiers dans un fichier temporaire (une ligne par chemin, UTF-8)
  et lance `chud-gui.exe --shell-convert <préréglage> --files-from <fichier>` puis rend la main
  immédiatement. **La DLL ne convertit jamais rien elle-même** : un plantage d'un moteur ne peut
  pas faire tomber l'Explorateur, et la DLL reste minuscule et auditable ;
- toute la sélection est transmise **en un seul appel** (pas un processus par fichier, pas de
  limite de 15 fichiers comme avec les verbes statiques du registre).

Enregistrement **par utilisateur**, sans droits administrateur :
`HKCU\Software\Classes\CLSID\{…}\InprocServer32` + `ExplorerCommandHandler` sur `*`,
`Directory` et `Directory\Background`. Installation et désinstallation propres par l'installeur
(et `chud shell install|uninstall|status` pour la version portable, avec confirmation).

### 2.2 Menu moderne de Windows 11

Le menu court de Windows 11 n'affiche que les commandes d'applications ayant une **identité de
paquet**. Pour y figurer : un **paquet MSIX « sparse »** (identité seule, l'application reste
installée normalement) qui déclare la même DLL. Ce paquet **doit être signé** par un certificat
reconnu par la machine → décision à prendre (voir ADR-017).

Sans paquet signé, la commande apparaît dans le menu classique (« Afficher plus d'options »,
Maj+F10) — c'est le cas de File Converter aujourd'hui.

### 2.3 Repli sans aucune inscription : « Envoyer vers »

Un raccourci `Chud Converter.lnk` dans `shell:sendto` ouvre l'application avec la sélection
(un seul appel, sélection multiple). Utile en mode portable ou en entreprise. Proposé, jamais
installé sans accord.

### 2.4 Application à instance unique

`chud-gui` est à instance unique : un second lancement transmet ses fichiers à l'instance déjà
ouverte (canal nommé local, protégé par l'identité de l'utilisateur), qui les ajoute à la file.
Plusieurs clics droits successifs alimentent donc la même file d'attente.

## 3. Linux

| Gestionnaire | Mécanisme | Sous-menus |
|---|---|---|
| Dolphin (KDE) | *service menu* `.desktop` dans `~/.local/share/kio/servicemenus/` | oui (`X-KDE-Submenu`) |
| Nautilus (GNOME) | extension `nautilus-python` (si présente), sinon scripts `~/.local/share/nautilus/scripts/` | oui / non |
| Nemo (Cinnamon) | actions `~/.local/share/nemo/actions/*.nemo_action` | non (une action par préréglage) |
| Thunar (Xfce) | actions personnalisées `~/.config/Thunar/uca.xml` | non |

Toutes appellent `chud-gui --shell-convert <préréglage> -- <fichiers…>` (arguments séparés, jamais
de ligne de shell construite à partir des noms de fichiers). Installation et retrait par
l'application, fichier par fichier, avec liste de ce qui a été écrit.

## 4. macOS (préparé)

Actions rapides Finder (extension *Finder Sync* ou *Quick Action* d'Automator générée), même
contrat d'appel.

## 5. Sécurité

- La DLL n'exécute qu'un seul programme : `chud-gui.exe` désigné par chemin absolu, inscrit à
  l'installation ; aucun interpréteur, aucune ligne de commande construite par concaténation.
- La liste des fichiers passe par un fichier temporaire créé avec `create_new` dans le dossier
  temporaire de l'utilisateur, supprimé après lecture.
- Aucune conversion ne démarre sur un préréglage inconnu ou modifié de façon invalide.
- Désinstallation complète : toutes les clés et fichiers créés sont journalisés et retirés.

## 6. Étapes de réalisation

1. Préréglages (modèle de données partagé CLI / interface / menu) et `chud-gui --shell-convert`.
2. Fenêtre de progression compacte + instance unique.
3. DLL `IExplorerCommand` + inscription HKCU + `chud shell install|uninstall|status`.
4. Paquet sparse signé pour le menu court de Windows 11 (si certificat disponible).
5. Linux : Dolphin, Nautilus, Nemo, Thunar.
6. Tests : inscription/désinscription dans une ruche de test, sélection de 1, 15, 500 fichiers,
   noms Unicode et commençant par un tiret, Explorateur réactif (< 50 ms pour construire le menu).
