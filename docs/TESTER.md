# Tester Chud Converter

Ce dossier est **portable** : grâce au fichier `portable`, les réglages et le cache restent dans
`data/` ici même. Rien n'est écrit dans votre profil. Pour tout effacer, supprimez le dossier.

Les fichiers du dossier `Exemples` sont générés pour l'occasion : vous pouvez les convertir,
les abîmer, les supprimer sans crainte. Testez aussi avec vos propres fichiers : l'original
n'est jamais modifié.

Prérequis : Windows 10 ou 11 (WebView2, déjà présent sur Windows 11). FFmpeg et 7-Zip sont
nécessaires pour la vidéo, l'audio et les archives (`winget install Gyan.FFmpeg` et
`winget install 7zip.7zip`). Les images et les données fonctionnent sans eux.

## Lancer

- **Application** : double-cliquez sur `chud-converter.exe`.
- **Avec des fichiers** : glissez des fichiers sur `chud-converter.exe` dans l'Explorateur ;
  ils s'ouvrent directement dans la liste.
- **Ligne de commande** : `chud.exe --help` dans un terminal ouvert dans ce dossier.

## À essayer, et ce qui doit se passer

| # | Action | Résultat attendu |
|---|---|---|
| 1 | Glisser `Exemples\vacances.mkv` dans la fenêtre, choisir **MP4** | Pastille jaune « Selon le contenu » ; vidéo et audio copiés sans réencodage ; perte des styles des sous-titres ASS annoncée **avant** de convertir |
| 2 | Cliquer **Convertir en MP4**, puis la flèche du rapport | `vacances.mp4` à côté de l'original ; « vérifié par décodage complet » ; l'original intact |
| 3 | Recommencer la même conversion | Un second fichier `vacances (1).mp4` ; rien n'est écrasé |
| 4 | Changer « Si le fichier existe déjà » en **Ne rien faire**, reconvertir | « Rien à faire » avec la raison |
| 5 | `Exemples\faux.jpg` | « extension trompeuse » : c'est un PNG ; les formats proposés sont ceux d'une image |
| 6 | `Exemples\abimee.mkv` (vidéo tronquée) en MP4 | **Échec** avec une cause claire, jamais un faux succès ; aucun fichier de sortie |
| 7 | `Exemples\film-long.mp4` en **WebM**, puis l'icône carrée « Annuler » | Progression et vitesse affichées ; après annulation, aucun fichier partiel |
| 8 | `Exemples\musique.wav` en **FLAC** puis le FLAC en **WAV** | Pastilles vertes « Sans perte » |
| 9 | `Exemples\photo.png` | Seulement des formats d'image et de vidéo : **jamais d'audio ni de sous-titres** |
| 10 | Ajouter `vacances.mkv` **et** `photo.png`, cocher les deux | Seuls les formats communs aux deux sont proposés |
| 11 | `Exemples\donnees.json` en **YAML**, **TOML** ou **CSV** | Conversion de données ; CSV annonce ce qui ne rentre pas dans un tableau |
| 12 | **Ajouter un dossier** → `Exemples\Dossier à compresser`, choisir **7z** ou **ZIP** | Archive créée avec l'arborescence et les noms accentués |
| 13 | Options : **Qualité vidéo**, **Hauteur maximale** = 480 | Le plan se met à jour ; la fidélité devient « Avec perte » si on réencode |
| 14 | Destination **Dans un dossier…** | Les résultats y sont écrits ; le réglage est mémorisé |
| 15 | Écran **Moteurs** | FFmpeg et 7-Zip « Prêt » avec version et emplacement ; commande d'installation pour les autres |
| 16 | Fermer la fenêtre pendant une conversion longue | Fermeture propre, aucun fichier partiel laissé |

## Signaler un problème

Notez le numéro du test, ce que vous avez fait et ce qui s'est affiché (une capture aide
beaucoup). Ouvrez le **rapport** de la conversion : il contient la cause exacte. Les problèmes
peuvent aussi être signalés sur https://github.com/BigFatDeuce/chud-converter/issues.
