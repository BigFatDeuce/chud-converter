# Tester Chud Converter

Ce dossier est **portable** : grâce au fichier `portable`, les réglages et le cache restent dans
`data/` ici même. Rien n'est écrit dans votre profil. Pour tout effacer, supprimez le dossier.

**Seule exception : le menu clic droit**, si vous l'activez (tests 21 à 25). Il ajoute des clés
au registre de votre compte Windows. Retirez-le **avant** de supprimer ce dossier : écran
**Préréglages** → **Retirer du menu** (ou `chud.exe shell uninstall`).

Les fichiers du dossier `Exemples` sont générés pour l'occasion : vous pouvez les convertir,
les abîmer, les supprimer sans crainte. Testez aussi avec vos propres fichiers : l'original
n'est jamais modifié.

Prérequis : Windows 10 ou 11 (WebView2, déjà présent sur Windows 11). Les moteurs (FFmpeg,
7-Zip, Pandoc, LibreOffice, Calibre…) s'installent depuis l'application elle-même (écran
**Moteurs**, tests 17a–17n et 40–58), sans terminal. Les images courantes et les données
fonctionnent sans eux.

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
| 15 | Écran **Moteurs** | FFmpeg et 7-Zip avec leur état, version et emplacement ; aucune commande à copier nulle part |
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

## Moteurs installés depuis l'application

Le paquet portable range tout dans son dossier `data\` : les moteurs installés ici n'apparaissent
que dans cette copie. Si FFmpeg ou 7-Zip est déjà installé sur votre PC, sa carte est « Prêt » et
propose seulement, en petit, **Installer aussi la version … de Chud Converter** : c'est le moyen
de tester l'installation sans rien toucher à votre installation (tests 17h–17i).

| # | Action | Résultat attendu |
|---|---|---|
| 17a | **Moteurs** → une carte « Non installé » avec un bouton **Installer (… Mo)** | Un encadré demande confirmation : version, taille, éditeur, licence, lien « Voir la publication d'origine ». **Annuler** referme sans rien télécharger |
| 17b | **Télécharger et installer** | Progression « Téléchargement : … sur … », puis « Vérification de l'empreinte SHA-256… », « Extraction… », « Essai de … » ; la carte passe à **Prêt**, origine « installé par Chud Converter » |
| 17c | Pendant le téléchargement de FFmpeg (115 Mo) : **Annuler l'installation** | Message « Installation … annulée : rien n'a été installé » ; la carte redevient « Non installé » |
| 17d | Après 17b, glisser une vidéo dans **Convertir** | Les formats vidéo/audio sont proposés sans relancer l'application ; la sélection et le format choisi sont gardés |
| 17e | Moteur installé → **Retirer** | Confirmation, puis la carte redevient « Non installé » ; le dossier `data\engines\<moteur>` ne contient plus de version |
| 17h | Moteur déjà installé sur votre PC → **Installer aussi la version … de Chud Converter** | La confirmation dit que votre version actuelle n'est pas modifiée et que celle de Chud Converter sera utilisée en priorité ; après installation, origine « installé par Chud Converter » |
| 17i | Puis **Retirer** | La carte redevient « Prêt », origine « installé sur le système », avec le chemin d'origine (rien n'a été modifié) |
| 17g | En ligne de commande : `chud engines`, puis `chud engines install 7zip` | La liste indique `chud engines install …` pour les moteurs installables ; l'installation affiche ses étapes |
| 17j | Seulement si FFmpeg n'est **pas** installé sur votre PC : glisser une vidéo dans **Convertir** | La ligne du fichier dit « aucune conversion possible… » et propose **installer FFmpeg (… Mo)** ; le clic ouvre **Moteurs** sur la confirmation (rien n'est téléchargé sans **Télécharger et installer**) ; une fois prêt, retour automatique à **Convertir**, fichier toujours sélectionné, formats vidéo proposés |
| 17m | Premier démarrage d'un dossier neuf (supprimez `data\` pour le revoir), s'il manque un moteur | Fenêtre « Bienvenue » : chaque moteur manquant avec son usage, sa taille, sa source et sa licence, un bouton **Installer** chacun et **Tout installer** s'il en manque plusieurs ; **Plus tard** ferme. Elle ne revient pas au démarrage suivant. Si rien ne manque, elle ne s'affiche pas |
| 17n | Cocher seulement un fichier qui demande un moteur absent | Panneau de droite : « Pour convertir « … », il faut FFmpeg. » avec **Installer FFmpeg (… Mo)** |
| 17l | Seulement s'il manque FFmpeg ou 7-Zip sur votre PC : écran **Convertir** | Un bandeau « Moteurs manquants : … » avec **Tout installer (… Mo)** ; la confirmation liste chaque moteur (taille, source, licence) ; après **Télécharger et installer**, chaque moteur s'installe à son tour, avec **Arrêter** ; le bandeau disparaît quand tout est prêt. La croix le masque jusqu'au prochain démarrage |
| 17k | Pendant l'installation de FFmpeg | Après la vérification, « Extraction : … % » avec une barre qui avance |

## Tous les moteurs (nouveau, 0.2.0)

Les onze autres moteurs s'installent maintenant depuis l'écran **Moteurs**, comme FFmpeg : un
bouton **Installer (… Mo)** chacun, ou **Tout installer**. Rien à taper. LibreOffice (≈ 380 Mo)
et Calibre (≈ 290 Mo) sont les plus longs.

| # | Action | Résultat attendu |
|---|---|---|
| 40 | Écran **Moteurs** | Treize moteurs, tous avec un bouton d'installation ou l'état « Prêt » ; plus de section « Bientôt pris en charge » |
| 41 | Installer **ImageMagick** ou **Ghostscript** sur un PC sans le runtime Visual C++ | La confirmation l'annonce (« le runtime Visual C++ de Microsoft sera installé ») ; Windows demande l'autorisation **une fois** ; un refus arrête proprement, rien n'est laissé à moitié |
| 42 | **Tout installer**, puis fermer et rouvrir l'application | Tous « Prêt », origine « installé par Chud Converter » ; tout est rangé dans `data\engines\` |
| 43 | `Exemples\notes.md` en **DOCX**, puis le DOCX en **PDF** | DOCX par Pandoc (titres, gras, tableau) ; PDF par LibreOffice ; rapport « vérifié » avec ce qui a été relu |
| 44 | `notes.md` en **HTML**, **ODT**, **EPUB** | Le HTML s'ouvre seul dans le navigateur (images intégrées) ; l'EPUB s'ouvre dans une liseuse |
| 45 | `Exemples\tableau.csv` en **XLSX**, puis le XLSX en **ODS** et en **PDF** | Tableau avec les accents justes (« Payé ») |
| 46 | `Exemples\livre.epub` en **MOBI**, **AZW3**, **FB2**, **PDF** | Calibre convertit ; deux chapitres et le sommaire gardés |
| 47 | `Exemples\document.pdf` (3 pages) en **PDF** : options **Linéariser** et **Pages** = `2-3` | qpdf, « Sans perte » ; le résultat a 2 pages, vérifié par `qpdf --check` |
| 48 | Pages = `5-9` sur ce PDF de 3 pages | Refus clair avant de convertir (pages hors du document), jamais un fichier vide |
| 49 | `document.pdf` en **PNG** avec **Résolution** = 300 et **Page** = 2 | Ghostscript ; une image nette de la page 2 |
| 50 | L'image PNG du test 49 en **TXT** (puis en **PDF**) | Tesseract : « Deuxieme page du document » reconnu ; remarque « relisez-le, des erreurs sont possibles » ; le PDF a un texte sélectionnable |
| 51 | Une image sans texte (`photo.png`) en **TXT** | **Échec** « aucun texte reconnu », pas un fichier vide réussi |
| 52 | `Exemples\photo.png` en **AVIF**, **HEIC** et **JPEG XL** | libvips ; décodé et vérifié |
| 53 | `photo.png` en **PSD**, puis le PSD en **PNG** | ImageMagick ; aller-retour correct |
| 54 | `Exemples\film-long.mp4` en **MKV** | Sans réencodage (« Sans perte ») ; le rapport donne le nombre de pistes recopiées |
| 55 | Sélectionner `vacances.mkv`, ouvrir le détail du fichier | Lignes « MediaInfo » en plus : conteneur, débit total, pistes audio et leur langue |
| 56 | Détail d'une photo de votre appareil (JPEG) | Lignes ExifTool : appareil, objectif, date, profil de couleur ; « EXIF » parmi les caractéristiques conservées ou perdues |
| 57 | Copier `notes.md` sous le nom `essai [1] %d.md`, le convertir en DOCX | Réussit : les `[ ]` et `%d` du nom ne sont jamais interprétés par un moteur |
| 58 | Ligne de commande : `chud.exe engines`, puis `chud.exe engines install qpdf` | Tous les moteurs listés comme installables ; installation avec ses étapes |

## Dossiers d'album et qualité d'origine (0.2.1)

Prenez un dossier de musique à vous (un album avec sa pochette, ou des MKV « gamerip »), si
possible sur **un autre disque** que celui de l'application.

| # | Action | Résultat attendu |
|---|---|---|
| 59 | **Ajouter un dossier** → le dossier de l'album | Les pistes s'ajoutent une par une ; message « Dossier … : N fichier(s) (audio) ajouté(s) ; laissé(s) de côté : cover.png » avec le bouton **Garder le dossier entier** |
| 60 | Cliquer **Garder le dossier entier** | Les pistes disparaissent, le dossier revient comme un seul élément (formats d'archive proposés) |
| 61 | Ajouter les pistes **et** la pochette avec **Ajouter des fichiers** | « Aucun format commun » puis un bouton par sorte : **Seulement les N fichier(s) : audio** ; un clic décoche la pochette |
| 62 | Une piste MKV qui ne contient que de l'audio (ex. un gamerip) | Plus d'« extension trompeuse » ; en haut de « Convertir en », **Qualité d'origine → Audio d'origine .mp3** (« la piste mp3 … ressort telle quelle, sans ré-encodage ») |
| 63 | Convertir les N pistes d'un coup avec **Audio d'origine**, puis avec le préréglage **MP3**, vers un autre disque | Toutes réussissent (plus d'« os error 32 ») ; avec le préréglage : « copié tel quel : le ré-encoder en 192 kbit/s n'améliorerait pas sa qualité » |
| 64 | `Exemples\vacances.mkv` | **Qualité d'origine** : audio d'origine, sous-titres d'origine (ASS) ; pas « Vidéo d'origine » (c'est déjà un MKV) |
| 65 | Une photo JPEG à vous (pas `faux.jpg`, qui est en réalité un PNG) | **Qualité d'origine → Image sans perte .png** |

## Images, livres et Linux (0.3.0)

Les moteurs ImageMagick, libvips, Calibre, Pandoc et LibreOffice doivent être installés (écran
**Moteurs**).

| # | Action | Résultat attendu |
|---|---|---|
| 66 | `Exemples\grande-image.png` (1024 × 512) → **ICO** | Réussit ; le plan et le rapport disent « redimensionnement 1024×512 → 256×128 (taille maximale d'une icône) » |
| 67 | Glisser `Exemples\image.tga` | Reconnu comme **Targa (TGA)**, plus comme « Curseur Windows » |
| 68 | `Exemples\image.pcx` | **APNG** n'est plus proposé (une image fixe ne peut pas devenir une animation) ; un GIF animé, lui, le propose toujours |
| 69 | `Exemples\grande-image.png` (avec transparence) → **Netpbm (PPM)** | Perte de la transparence annoncée **avant** ; le `.ppm` obtenu s'ouvre dans un autre logiciel (GIMP, IrfanView…) et se reconvertit en PNG |
| 70 | `Exemples\photo.png` → **Radiance HDR**, puis le `.hdr` obtenu → **PNG** | Les deux réussissent (avant : « ce fichier ne peut pas être analysé » pour le HDR écrit sous Windows en français) |
| 71 | `Exemples\bd.cbz` → **Texte brut (TXT)** | Échec clair : « aucun texte à extraire : ce livre ne contient que des images » (avant : « Calibre a produit un fichier vide ») |
| 72 | `Exemples\lettre.odt` (créée par LibreOffice, avec une image) → **MOBI** ou **EPUB** | Réussit, en passant par LibreOffice ou Pandoc (avant : « KeyError: 'Bitmap_20_1' » de Calibre) |
| 73 | `Exemples\notes-image.md` (fait référence à `couverture.jpg`) → **Jupyter (ipynb)** | Réussit ; le rapport dit « images liées non reprises (remplacées par leur légende) » |
| 74 | Linux (Fedora, Nobara) : l'AppImage, écran **Moteurs** | Les moteurs installés par le système sont « prêts » (plus de « symbol lookup error … nghttp2 ») ; **Installer** ExifTool fonctionne (paquet `perl-Image-ExifTool`) |
| 75 | Linux (Fedora, Nobara) avec LibreOffice sans Calc (`sudo dnf remove libreoffice-calc`) | Écran **Moteurs** : « Installation incomplète, absent : Calc (classeurs) » et un bouton **Ajouter Calc (classeurs)** ; un `.xlsx` propose ce bouton au lieu de formats ; après l'ajout (mot de passe demandé), le `.xlsx` se convertit |

## Nouvelle interface « espace profond » (0.4.0)

Toute l'interface a été redessinée : fond étoilé, panneaux de verre, navigation repensée. Rien
ne doit avoir disparu : chaque bouton et chaque option d'avant est toujours là.

| # | Action | Résultat attendu |
|---|---|---|
| 76 | Parcourir les six écrans (**Convertir**, **Outils**, **Préréglages**, **Moteurs**, **Réglages**, **À propos**) | Même style partout ; l'écran actif est marqué d'un trait lumineux à gauche ; aucun texte coupé ou illisible |
| 77 | **Ctrl+1** à **Ctrl+6**, puis **Ctrl+O** | Les raccourcis ouvrent les écrans dans l'ordre du menu ; **Ctrl+O** ouvre le choix de fichiers |
| 78 | Rétrécir la fenêtre au minimum, puis l'agrandir | En dessous d'environ 1100 px de large, le menu ne garde que les icônes (nom au survol) ; rien ne déborde ; tout défile |
| 79 | **Convertir** : ajouter des fichiers, choisir un format, convertir | Les trois étapes en haut (Fichiers, Format, Conversion) s'allument au fur et à mesure ; barre de progression lumineuse pendant la conversion |
| 80 | **Réglages → Apparence → Thème** : Système, Sombre, Clair | Le changement est immédiat ; en clair, fond pâle et texte foncé, tout reste lisible ; le choix est gardé au prochain démarrage |
| 81 | **Réglages → Apparence → Animations** : Désactivées | Plus rien ne bouge : ni étoiles, ni orbites, ni transitions, ni reflets ; les barres « en cours » deviennent des hachures fixes |
| 82 | Animations : **Réduites** ; puis **Automatiques** avec, dans Windows, *Paramètres → Accessibilité → Effets visuels → Effets d'animation* désactivés | Fond immobile, seulement de courts fondus ; en automatique, l'application suit ce réglage de Windows |
| 83 | Menu clic droit (préréglage) pendant que l'application est ouverte, après avoir changé le thème | La fenêtre de conversion rapide a le même thème et les mêmes animations que la fenêtre principale |
| 84 | Premier démarrage (dossier `data` vidé) | L'accueil s'affiche en fenêtre centrée ; les boutons **Tout installer** et **Plus tard** restent visibles même en faisant défiler la liste des moteurs |
| 85 | Laisser l'application ouverte quelques minutes sur l'écran **Convertir** | Le processeur reste au repos (Gestionnaire des tâches : près de 0 %) ; l'animation du fond ne ralentit pas l'ordinateur |

## Outils : PDF et archives (0.5.0)

L'écran **Outils** réunit ce qui n'est pas une conversion. Il faut **qpdf** pour les PDF et **7-Zip**
pour les archives ; s'ils manquent, l'écran propose de les installer.

| # | Action | Résultat attendu |
|---|---|---|
| 86 | **Outils** sans qpdf, carte **Fusionner des PDF** | « moteur à installer » ; bouton **Installer qpdf** à droite ; une fois installé, retour sur l'écran Outils |
| 87 | **Fusionner des PDF** : glisser `rapport.pdf`, `annexe 10.pdf` et `annexe 2.pdf` (dossier `Exemples`) sur l'écran | Trois fichiers numérotés ; à droite « 3 fichiers, 8 pages » et le nom du résultat |
| 88 | **Trier par nom**, puis les flèches pour mettre `rapport.pdf` en premier | `annexe 2` passe avant `annexe 10` ; l'ordre affiché est celui des pages du résultat |
| 89 | Lancer la fusion, ouvrir le résultat dans un lecteur PDF | `rapport - fusion.pdf`, 8 pages ; panneau des signets : un signet par fichier, et sous « rapport » ses trois chapitres ; chaque signet mène à la bonne page |
| 90 | Ajouter `photo.png` à la liste de la fusion | Affiché grisé, « laissé de côté — PNG : ce n'est pas un PDF » ; la fusion se fait sans lui |
| 91 | Option **Signets** sur « aucun », nom « Dossier complet » | Le rapport annonce la perte des signets **avant** de lancer ; le fichier s'appelle `Dossier complet.pdf` |
| 92 | **Diviser un PDF** : `rapport.pdf` seul, une page par fichier | Dossier `rapport - pages` avec 4 PDF d'une page (`rapport - p1.pdf`…) |
| 93 | Diviser avec les plages `1-2; 3-z` | Deux fichiers de 2 pages ; chacun ne contient que ses propres signets |
| 94 | Plage `1-99` | Refusée tout de suite à droite, le bouton reste grisé |
| 95 | **Pivoter un PDF** : `annexe 2.pdf` (page couchée), 270° | `annexe 2 - pivoté.pdf` à l'endroit ; « Sans perte » ; les signets restent |
| 96 | Un PDF protégé par mot de passe (à vous) dans Fusionner ou Diviser | Refusé avec la raison ; aucun fichier créé |
| 97 | **Créer une archive** : plusieurs fichiers et `Dossier à compresser` | Une seule archive ; « test d'intégrité réussi et … fichiers identiques en nom et en taille » dans le rapport détaillé |
| 98 | Archive avec deux fichiers du même nom venant de dossiers différents | Le rapport annonce « … devient « nom (2) » » ; les deux sont dans l'archive |
| 99 | **Convertir** : cocher deux PDF | Sous « Outils », à droite : Fusionner, Diviser, Pivoter, Créer une archive ; un clic ouvre l'outil avec ces fichiers |
| 100 | Relancer la même fusion, puis avec « Remplacer l'ancien résultat » | D'abord « … (1).pdf », puis remplacement ; jamais un des fichiers d'origine |
| 101 | `chud tool`, puis `chud tool pdf-merge "Exemples\rapport.pdf" "Exemples\annexe 2.pdf" --dry-run` | Liste des outils et de leurs options ; aperçu sans rien écrire |
| 102 | **Moteurs → Calibre → Installer** | Le téléchargement fonctionne de nouveau (erreur 404 corrigée) |

## Corrections après relecture (nouveau, 0.5.1)

Une relecture indépendante du code a trouvé quelques défauts, corrigés ici. Rien ne change à
l'écran : ces essais vérifient que tout se passe comme avant, en plus sûr.

| # | Action | Résultat attendu |
|---|---|---|
| 103 | **Diviser un PDF** : `rapport.pdf`, une page par fichier, en gardant l'Explorateur ouvert sur le dossier de sortie | Le dossier `rapport - pages` apparaît d'un coup avec ses 4 PDF, jamais à moitié rempli |
| 104 | **Convertir** une archive (`bd.cbz` ou un ZIP à vous) en **Dossier** | Même chose : le dossier apparaît complet ; aucun dossier `.~chud-….part` ne reste à côté |
| 105 | **Convertir** `lettre.odt` en DOCX, puis en XLSX si proposé ; ouvrir le résultat | Le DOCX s'ouvre dans Word ou LibreOffice avec le texte ; le rapport détaillé dit « conteneur ZIP intact » |
| 106 | Lancer une longue conversion vidéo, puis **Annuler** | Arrêt en une seconde environ ; aucun fichier partiel ; `ffmpeg.exe` disparaît du Gestionnaire des tâches |

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
