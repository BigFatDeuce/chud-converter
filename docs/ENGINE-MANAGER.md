# Gestionnaire de moteurs et mises à jour

## 1. Objectifs

Détecter, vérifier, installer, mettre à jour, épingler, désactiver et désinstaller les outils
externes (FFmpeg, 7-Zip, LibreOffice, Pandoc, Calibre…) **sans droits administrateur**, de façon
**vérifiable, réversible et transparente**, et sans jamais toucher à une installation gérée par
le système.

## 2. Manifeste de moteur

Un fichier TOML par moteur dans `crates/chud-enginemgr/data/engines/` :

```toml
id = "ffmpeg"
name = "FFmpeg"
homepage = "https://ffmpeg.org"
license = "LGPL-2.1-or-later / GPL-3.0-or-later selon la variante"
categories = ["video", "audio", "image", "subtitle"]
executables = ["ffmpeg", "ffprobe"]          # le premier est l'exécutable principal

[detect]
version_args = ["-hide_banner", "-version"]
version_regex = 'ffmpeg version (?:n)?([0-9][^\s]*)'
known_paths_windows = []
known_paths_linux = ["/usr/bin/ffmpeg"]

[[source]]                                    # sources de téléchargement par plateforme
platform = "windows-x86_64"
kind = "github-release"
repo = "BtbN/FFmpeg-Builds"
asset = 'ffmpeg-n(?P<version>[0-9.]+)-[0-9]+-g[0-9a-f]+-win64-gpl-[0-9.]+\.zip'
checksums_asset = "checksums.sha256"
bin_dir = "*/bin"
approx_size_mb = 140
```

Types de sources prévus : `github-release`, `url` (URL + SHA-256 épinglé), `index` (index signé
du projet), `system-package` (instructions apt/dnf/pacman/winget affichées, jamais exécutées
sans accord).

## 3. Détection

Ordre de recherche (le premier fonctionnel gagne, sauf préférence utilisateur) :

1. chemin personnalisé défini par l'utilisateur (`Custom`) ;
2. installation gérée par l'application (`Managed`, version `current`) ;
3. emplacements connus (`C:\Program Files\7-Zip\7z.exe`, `/usr/bin/7zz`…) (`System`) ;
4. `PATH` (`System`).

Pour chaque candidat : exécution de la commande de version (délai 10 s, processus confiné),
extraction de la version, puis **sonde de capacités** propre au moteur (ex. `ffmpeg -muxers
-demuxers -encoders`). Un moteur qui ne répond pas est marqué `Broken` avec la raison.

Le chemin absolu résolu est conservé ; l'exécution ne refait jamais de recherche dans le `PATH`.

## 4. Installation

```
<données>/engines/<id>/
    versions/<version>/...      fichiers extraits
    current                     fichier texte : version active
    pinned                      (optionnel) version épinglée
    install.json                origine, URL, SHA-256, date, taille, licence
```

Étapes :
1. Résolution de la version cible (index signé ou API de la source) selon le canal (stable/bêta)
   et l'épinglage.
2. Affichage à l'utilisateur : version, taille, licence, source, notes de version.
3. Téléchargement en flux vers un fichier temporaire, avec calcul SHA-256 à la volée, limite de
   taille, reprise possible.
4. **Vérification** : SHA-256 attendu (index signé ou fichier de sommes amont) et signature si
   l'amont en publie. Échec ⇒ suppression et refus.
5. Extraction **contrôlée** (mêmes protections que les archives utilisateur : pas de traversée,
   pas de liens sortants, limites de taille) dans `versions/<version>.partial`, puis renommage.
6. **Vérification fonctionnelle** : commande de version + conversion de test minimale.
7. Bascule du pointeur `current`. L'ancienne version est conservée (2 versions par défaut) ⇒
   **retour arrière** instantané.

## 5. Mises à jour

| Réglage | Valeurs |
|---|---|
| Vérification | jamais / au démarrage / quotidienne / hebdomadaire |
| Téléchargement | manuel / automatique |
| Installation | manuelle / automatique (si l'utilisateur l'autorise) |
| Canal | stable / bêta (par moteur) |
| Épinglage | version bloquée par moteur |
| Actions | tout mettre à jour, mise à jour individuelle, revenir à la version précédente |

Les installations `System` ne sont jamais mises à jour par l'application ; l'interface indique
comment les mettre à jour (gestionnaire de paquets) ou propose d'installer une version gérée
**à côté**.

## 6. Index signé du projet (ADR-007)

Fichier `engines-index.json` publié avec sa signature `engines-index.json.minisig` :

```json
{
  "schema": 1,
  "generated": "2026-11-18T00:00:00Z",
  "engines": {
    "ffmpeg": {
      "stable": [{
        "version": "9.0.2",
        "platform": "windows-x86_64",
        "url": "https://github.com/BtbN/FFmpeg-Builds/releases/download/…",
        "sha256": "…",
        "size": 146800640,
        "license": "GPL-3.0-or-later",
        "notes_url": "https://ffmpeg.org/…"
      }]
    }
  }
}
```

La clé publique est compilée dans l'application. La CI du projet construit l'index en
vérifiant les sommes/signatures amont (et, pour les amonts sans sommes publiées, en
téléchargeant depuis plusieurs emplacements officiels et en comparant).

## 7. Distinction avec les mises à jour de l'application

L'application elle-même se met à jour par l'updater Tauri (paquet signé, canal stable/bêta) :
deux systèmes, deux écrans, deux réglages (ADR-015).

## 8. État d'implémentation

| Fonction | État |
|---|---|
| Manifestes, détection (Custom/Managed/connus/PATH), sonde de version | ✅ implémenté et testé |
| Sonde de capacités FFmpeg / 7-Zip | ✅ implémenté et testé |
| Désactivation d'un moteur | ✅ (registre) |
| Téléchargement, vérification SHA-256, extraction contrôlée, installation versionnée | ⏳ prochaine itération |
| Index signé, notes de version, mise à jour automatique, retour arrière | ⏳ |
