# Tester Chud Converter

Ce dossier est **portable** : grâce au fichier `portable`, les réglages et le cache restent dans
`data/` ici même. Rien n'est écrit dans votre profil. Pour tout effacer, supprimez le dossier.

**Seule exception : le menu clic droit**, si vous l'activez (tests 21 à 25). Il ajoute des clés
au registre de votre compte Windows. Retirez-le **avant** de supprimer ce dossier : écran
**Préréglages** → **Retirer du menu** (ou `chud.exe shell uninstall`).

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

## Audio et vérifications (0.1.1)

| # | Action | Résultat attendu |
|---|---|---|
| 16a | `Exemples\album.flac` (3 min) en **AAC**, puis en **AMR** | **Terminé**, « vérifié par décodage complet » ; le rapport donne la durée décodée (≈ 187 s) ; AMR annonce le passage en mono 8 kHz |
| 16b | `album.flac` en **Opus**, **CAF**, **WavPack**, **WMA** | Tout réussit ; WavPack et CAF « Sans perte », avec « échantillons identiques à la source (MD5) » dans le rapport |
| 16c | `Exemples\chanson.ogg` en **MP3** | Le titre « Été » et l'artiste sont conservés (propriétés du fichier) ; vers **AAC** ou **AMR**, la perte des balises est annoncée **avant** |
| 16d | `Exemples\coupee.flac` (moitié d'un FLAC) en **MP3** | Terminé avec la remarque « source incomplète : seules … s sur les … s annoncées sont lisibles » ; en **FLAC**, échec qui dit que **la source** est endommagée |
| 16e | `Exemples\pas-une-video.mp4` | Aucun format proposé : « ce fichier ne peut pas être analysé… » ; jamais une liste de conversions vouées à l'échec |
| 16f | Une conversion qui échoue | Le libellé distingue « Vérification échouée, aucun fichier conservé », « Source illisible ou endommagée » et « Échec de la conversion », avec la cause exacte dessous |

## Moteurs installés depuis l'application (nouveau)

Le paquet portable range tout dans son dossier `data\` : les moteurs installés ici n'apparaissent
que dans cette copie. Si FFmpeg ou 7-Zip est déjà installé sur votre PC, il reste utilisé et
aucun bouton « Installer » n'apparaît pour lui (c'est voulu : Chud Converter ne remplace jamais
une installation existante).

| # | Action | Résultat attendu |
|---|---|---|
| 17a | **Moteurs** → une carte « Non installé » avec un bouton **Installer (… Mo)** | Un encadré demande confirmation : version, taille, éditeur, licence, lien « Voir la publication d'origine ». **Annuler** referme sans rien télécharger |
| 17b | **Télécharger et installer** | Progression « Téléchargement : … sur … », puis « Vérification de l'empreinte SHA-256… », « Extraction… », « Essai de … » ; la carte passe à **Prêt**, origine « installé par Chud Converter » |
| 17c | Pendant le téléchargement de FFmpeg (115 Mo) : **Annuler l'installation** | Message « Installation … annulée : rien n'a été installé » ; la carte redevient « Non installé » |
| 17d | Après 17b, glisser une vidéo dans **Convertir** | Les formats vidéo/audio sont proposés sans relancer l'application ; la sélection et le format choisi sont gardés |
| 17e | Moteur installé → **Retirer** | Confirmation, puis la carte redevient « Non installé » ; le dossier `data\engines\<moteur>` ne contient plus de version |
| 17f | Une carte sans paquet vérifié (Pandoc, LibreOffice…) | La commande du gestionnaire de paquets s'affiche, **sans dépasser de la carte** même si elle est longue ; bouton copier |
| 17g | En ligne de commande : `chud engines`, puis `chud engines install 7zip` | La liste indique `chud engines install …` pour les moteurs installables ; l'installation affiche ses étapes |

## Écrans qui défilent (corrigé)

| # | Action | Résultat attendu |
|---|---|---|
| 18a | **Préréglages**, fenêtre réduite en hauteur | La liste défile jusqu'au dernier préréglage et jusqu'au menu clic droit |
| 18b | Même essai sur **Moteurs**, **Réglages**, **À propos**, et le panneau de droite de **Convertir** | Tout défile jusqu'en bas, rien n'est coupé |

## Préréglages et menu clic droit

| # | Action | Résultat attendu |
|---|---|---|
| 17 | Ajouter `Exemples\vacances.mkv` | Sous « Convertir en », une rangée **Préréglages** (MP4, MP4 léger, WebM, GIF, MP3…) ; jamais « MKV » pour un MKV |
| 18 | Cliquer **MP4 léger (720p)** | Le format et les options se remplissent, le plan passe « Avec perte » ; **rien n'est converti** avant « Convertir » |
| 19 | Choisir un format et des options, puis **Enregistrer comme préréglage…** | Le nouveau préréglage apparaît dans la rangée et dans l'écran **Préréglages** (étiquette « personnel ») |
| 20 | Écran **Préréglages** : décocher, renommer (**Modifier**), monter/descendre, supprimer | Chaque changement est enregistré ; la suppression demande une confirmation ; **Rétablir les préréglages fournis** garde une copie de l'ancien fichier |
| 21 | Écran **Préréglages** → **Ajouter au menu** | « Installé pour … types de fichiers » ; aucune demande de droits d'administrateur |
| 22 | Dans l'Explorateur, clic droit sur `Exemples\photo.png` (Windows 11 : **Afficher plus d'options**) → **Chud Converter** → **WebP** | Petite fenêtre « conversion » ; `photo.webp` à côté de l'original ; la fenêtre se ferme seule après quelques secondes si tout a réussi (survoler la fenêtre la garde ouverte) |
| 23 | Sélectionner `musique.wav` et `vacances.mkv` ensemble → **Chud Converter** → **MP3** | Une seule fenêtre avec les deux conversions |
| 24 | Clic droit sur le dossier `Dossier à compresser` → **Chud Converter** → **Compresser en ZIP** | Archive créée à côté du dossier |
| 25 | **Retirer du menu** | Le menu disparaît de l'Explorateur ; `chud.exe shell status` répond « non installé » |
| 26 | Terminal : `chud.exe convert Exemples\musique.wav --preset flac` puis `chud.exe presets Exemples\photo.png` | Conversion par préréglage ; la liste des préréglages proposés pour une image (sans « PNG ») |

## Langue (français / anglais)

| # | Action | Résultat attendu |
|---|---|---|
| 27 | Premier lancement | L'application parle la langue de Windows : français si Windows est en français, anglais sinon |
| 28 | Écran **Réglages** → **English**, avec `vacances.mkv` dans la liste et un format choisi | Tout passe en anglais **sans redémarrer** : menus, format du fichier (« Video »), cibles, plan, options, préréglages (« Light MP4 (720p) »), écran Moteurs ; le fichier et le format choisis restent |
| 29 | Convertir en anglais, puis revenir en **Français** | Le nouveau rapport est en anglais ; après le retour, l'interface est en français, l'ancien rapport garde sa langue |
| 30 | **Langue du système** | Retour à la langue de Windows ; le choix est mémorisé après fermeture |
| 31 | Terminal : `chud.exe language en`, puis `chud.exe --help` et `chud.exe plan Exemples\vacances.mkv --to mp4` | Aide et plan en anglais ; `chud.exe language` affiche le choix et le fichier où il est enregistré |
| 32 | `chud.exe --lang fr detect Exemples\faux.jpg` | Réponse en français pour cette seule commande ; le réglage ne change pas |
| 33 | Avec le menu clic droit installé (test 21), changer de langue dans **Réglages** | Les entrées du menu (« Compress to ZIP », « Open in Chud Converter… ») suivent la nouvelle langue. **Retirez le menu ensuite** (test 25) |

## Signaler un problème

Notez le numéro du test, ce que vous avez fait et ce qui s'est affiché (une capture aide
beaucoup). Ouvrez le **rapport** de la conversion : il contient la cause exacte. Les problèmes
peuvent aussi être signalés sur https://github.com/BigFatDeuce/chud-converter/issues.
