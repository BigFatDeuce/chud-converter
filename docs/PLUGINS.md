# Système de plugins (conception — phase 3)

## 1. Objectifs

Ajouter moteurs, formats, convertisseurs, analyseurs et optimiseurs **sans mise à jour de
l'application**, sans jamais laisser un plugin inconnu exécuter du code sans contrôle.
Le système fonctionne sans catalogue communautaire.

## 2. Trois niveaux (ADR-011)

### Niveau 1 — Adaptateur déclaratif (TOML)
Décrit un outil en ligne de commande déjà installé ou téléchargeable via le gestionnaire de
moteurs. Aucun code : l'application construit le vecteur d'arguments à partir d'un modèle.

```toml
[plugin]
id = "org.example.cjxl"
name = "libjxl (cjxl/djxl)"
version = "1.0.0"
license = "BSD-3-Clause"
authors = ["Exemple"]
engine = "libjxl"                 # moteur du gestionnaire requis

[[conversion]]
from = ["png", "jpeg", "gif", "pnm", "exr"]
to = "jxl"
fidelity = "depends"              # sans perte si distance=0 ou JPEG recompressé
program = "cjxl"
args = ["{input}", "{output}", "--distance={opt.distance}", "--effort={opt.effort}"]

[[conversion.option]]
key = "distance"
kind = { float = { min = 0.0, max = 15.0 } }
default = 1.0
```

Garanties : `{input}`/`{output}` sont des chemins du staging, jamais du texte libre ; seules les
clés d'options déclarées et validées par leur type sont substituées ; aucune substitution dans
le nom du programme.

### Niveau 2 — WebAssembly (Wasmtime + Component Model)
Pour les convertisseurs et analyseurs écrits dans n'importe quel langage compilant vers
`wasm32-wasip2`. Interface WIT (extrait) :

```wit
package chud:plugin@0.1.0;

interface converter {
  record capability { from: list<string>, to: string, fidelity: fidelity }
  enum fidelity { lossless, lossy, depends }
  capabilities: func() -> list<capability>;
  convert: func(input: input-stream, to: string, options: list<tuple<string, string>>,
                output: output-stream) -> result<_, string>;
}

world plugin { export converter; }
```

Bac à sable : aucun accès réseau, aucun système de fichiers (flux fournis uniquement), mémoire
plafonnée, interruption par epoch (délai), pas d'horloge haute précision.

### Niveau 3 — Moteurs intégrés (Rust)
Compilés dans l'application, réservés au projet.

## 3. Manifeste et métadonnées

Nom, identifiant inversé, auteur, description, capacités, dépendances (moteurs, version
minimale de l'application), version (SemVer), licence (SPDX), origine (URL), **permissions**
demandées (`engines: [...]`, `network: false` toujours pour le niveau 2), historique des versions.

## 4. Confiance et cycle de vie

- Paquet `.chudplugin` (ZIP) + signature minisign ; clés de confiance : clé officielle du projet
  + clés ajoutées explicitement par l'utilisateur.
- Plugin non signé : installable uniquement en « mode développeur », signalé en permanence.
- Vérification de compatibilité (`app_version` SemVer) avant activation.
- Installation, mise à jour, désactivation, désinstallation depuis l'écran Plugins.
- Catalogue communautaire : index JSON signé (même mécanisme que l'index des moteurs), optionnel.
