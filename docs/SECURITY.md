# Sécurité

Le logiciel manipule des fichiers d'origine inconnue avec des outils puissants. Principe
directeur : **tout fichier d'entrée est hostile** ; tout moteur externe est potentiellement
vulnérable ; aucune donnée ne quitte la machine sans demande explicite.

## 1. Menaces et parades

| Menace | Parade |
|---|---|
| Injection de commande via un nom de fichier | Jamais de shell. Programme absolu + arguments séparés (`std::process::Command`). |
| Nom de fichier commençant par `-` interprété comme option | Chemins toujours **absolus** ; pour FFmpeg, préfixe `file:`. |
| Noms interprétés comme protocoles par FFmpeg (`concat:`, `http:`, `subfile,`) | Préfixe `file:` + `-protocol_whitelist file,pipe`. |
| Playlists HLS / concat / fichiers `.ffconcat` lisant des fichiers locaux ou distants (exfiltration) | Démultiplexeurs `hls`, `concat` et formats de liste refusés pour les entrées non confirmées ; liste blanche de protocoles. |
| Détournement du `PATH` (DLL/EXE plantés dans le dossier courant) | Résolution unique du chemin absolu par le gestionnaire de moteurs ; répertoire de travail = staging. |
| Bombes de décompression (archives) | Pré-inventaire avant extraction ; limites : taille totale décompressée, ratio par entrée, nombre d'entrées, profondeur d'imbrication ; surveillance pendant l'extraction. |
| Bombes de décompression (images) | Limites de dimensions et d'allocation (`image::Limits`), `policy.xml` ImageMagick, `VIPS_` limites. |
| Traversée de chemins (`../`, chemins absolus, lecteurs `C:`, UNC, flux ADS `:`) | Validation de chaque entrée avant extraction ; refus explicite de l'archive. |
| Liens symboliques / physiques sortants, fichiers spéciaux | Non extraits par défaut ; vérification post-extraction (aucun point d'analyse sortant). |
| Noms réservés Windows (`CON`, `NUL`, `COM1`…), collisions de casse | Renommage sûr et signalé. |
| ImageTragick (délégués `MVG`, `MSL`, `URL`, `EPHEMERAL`, `TEXT`) | `policy.xml` durci fourni par l'application et passé via `MAGICK_CONFIGURE_PATH`. |
| PostScript malveillant | Ghostscript toujours en `-dSAFER`, jamais d'option `-dNOSAFER`. |
| Documents bureautiques avec liens externes (fuite NTLM/SSRF) | LibreOffice en profil isolé, mise à jour des liens désactivée, macros désactivées. |
| Pandoc : inclusion de fichiers locaux | `--sandbox`. |
| Moteur compromis / vulnérable | Processus confiné (Job Object/limite mémoire, délai), sortie vérifiée, version épinglable, mise à jour rapide. |
| Binaires de mise à jour falsifiés | Application : paquets signés Ed25519 (clé publique intégrée au programme), version signée comparée à la version annoncée, refus sinon (testé avec un paquet substitué). Moteurs : index signé Ed25519 + SHA-256 (à venir). |
| Vol de la clé de signature | Clé privée hors du dépôt, seulement dans un secret GitHub utilisé par le flux de publication ; perte = plus de mise à jour automatique (réinstallation manuelle). |
| Épuisement de ressources | Pool borné, poids des tâches, délais, limite mémoire par processus, contrôle d'espace disque avant écriture. |
| Plugins malveillants | Pas de code natif tiers ; WebAssembly sans accès réseau/FS hors flux fournis ; signatures. |

## 2. Intégrité des données utilisateur

- Le fichier source n'est **jamais** modifié ni supprimé par défaut.
- Écriture dans un staging, vérification, puis déplacement atomique ; jamais de fichier final
  partiel visible.
- Conflits de noms : renommage par défaut ; écrasement seulement sur demande explicite et jamais
  du fichier source lui-même.
- Suppression (doublons, nettoyage) : corbeille du système par défaut, confirmation explicite,
  suppression définitive uniquement sur demande expresse.
- Réparation : toujours sur une **copie**, résultat annoncé comme partiel quand il l'est.

## 3. Confidentialité

- Traitement 100 % local. Les seules connexions réseau sont : vérification/téléchargement des
  moteurs, des plugins et des mises à jour de l'application — chacune désactivable.
- Aucune télémétrie. Aucun nom de fichier, chemin ni contenu dans les requêtes réseau.

## 4. Signalement de vulnérabilités

Créer un rapport privé (GitHub Security Advisories) une fois le dépôt public. Ne pas ouvrir
d'issue publique pour une vulnérabilité exploitable.

## 5. État d'implémentation

| Mesure | État |
|---|---|
| Processus sans shell, chemins absolus, `file:` + liste blanche de protocoles FFmpeg | ✅ |
| Confinement d'arbre de processus (Job Object Windows / groupe Unix), délais, annulation | ✅ |
| Limite mémoire par processus (Job Object Windows) | ✅ ; Unix `setrlimit` ⏳ |
| Staging + vérification + finalisation atomique, source jamais modifiée | ✅ |
| Validation d'archives avant extraction (traversée, liens, taille, ratio, nombre) | ✅ (7-Zip) |
| Limites d'images (`image::Limits`) | ✅ |
| `policy.xml` ImageMagick, profils LibreOffice isolés, `--sandbox` Pandoc | ⏳ avec ces moteurs |
| Index signé des moteurs | ⏳ |
| Fuzzing de la détection et des validations | ⏳ |
