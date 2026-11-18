# Workflows (conception — phase 3)

## 1. Principes

- Un workflow est un **graphe orienté acyclique** d'opérations sérialisé en JSON (partageable).
- Deux vues sur le même modèle : **recette linéaire** (liste d'étapes, pour débutants) et
  **graphe de nœuds** (embranchements, conditions, gestion d'erreurs) via Svelte Flow.
- Exécution par le même moteur de tâches que les conversions simples (staging, vérification,
  finalisation) : un workflow n'a pas de chemin d'exécution « à part ».
- **Simulation** (dry-run) : chaque exécution peut d'abord être simulée et afficher les fichiers
  qui seraient produits, déplacés ou ignorés.

Inspirations étudiées : Node-RED, n8n, ComfyUI, nœuds de Blender (graphe) ; XnConvert,
Hazel, DropIt (recettes et règles de classement).

## 2. Nœuds prévus

| Famille | Nœuds |
|---|---|
| Entrées | Fichiers déposés, dossier surveillé, liste de fichiers |
| Analyse | Détection, inspection, lecture de métadonnées |
| Conditions | Filtre par catégorie/format/taille/dimensions/durée/métadonnée, aiguillage (switch) |
| Opérations | Convertir, optimiser, redimensionner, extraire l'audio, extraire des images, OCR, nettoyer les métadonnées |
| Fichiers | Renommer (modèle à jetons), déplacer, copier, mettre en corbeille, créer une archive |
| Contrôle | Embranchement parallèle, jonction, sur erreur, notification |

Ports typés : `fichier`, `liste de fichiers`, `rapport`. Une connexion invalide est refusée
par l'éditeur.

## 3. Exemple sérialisé

```json
{
  "schema": 1,
  "name": "Vidéos entrantes",
  "nodes": [
    {"id": "in",  "type": "input.watch", "params": {"path": "D:/Entrant"}},
    {"id": "f",   "type": "filter.category", "params": {"category": "video"}},
    {"id": "mp4", "type": "op.convert", "params": {"to": "mp4", "profile": "video-1080p"}},
    {"id": "mp3", "type": "op.convert", "params": {"to": "mp3", "options": {"audio_bitrate": "192k"}}},
    {"id": "o1",  "type": "file.move", "params": {"dest": "D:/Vidéos"}},
    {"id": "o2",  "type": "file.move", "params": {"dest": "D:/Audio"}}
  ],
  "edges": [["in","f"],["f","mp4"],["mp4","o1"],["mp4","mp3"],["mp3","o2"]],
  "on_error": {"default": "stop-branch"}
}
```
