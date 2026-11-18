#!/usr/bin/env python3
"""Construit un dossier portable prêt à tester : portable/Chud Converter/.

- compile l'interface (npm) puis `chud-converter` et `chud` en version release ;
- crée le fichier `portable` : réglages et cache restent dans `data/` à côté des programmes ;
- génère des fichiers d'exemple réels avec FFmpeg (s'il est installé) ;
- copie le guide de test docs/TESTER.md.

Toutes les commandes sont lancées avec des arguments séparés, jamais via un shell.
Usage : python scripts/package_test.py [--skip-build]
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "portable" / "Chud Converter"
EXE = ".exe" if os.name == "nt" else ""


def run(args: list[str], cwd: Path = ROOT) -> None:
    print("›", " ".join(args), flush=True)
    subprocess.run(args, cwd=cwd, check=True)


def tool(name: str) -> str | None:
    return shutil.which(name)


def build() -> None:
    npm = tool("npm") or sys.exit("npm introuvable : installez Node.js")
    cargo = tool("cargo") or sys.exit("cargo introuvable : installez Rust (rustup)")
    desktop = ROOT / "apps" / "desktop"
    if not (desktop / "node_modules").exists():
        run([npm, "ci"], desktop)
    run([npm, "run", "build"], desktop)
    # L'interface est intégrée au programme : forcer sa ré-intégration après chaque build.
    (desktop / "src-tauri" / "build.rs").touch()
    run([cargo, "build", "--release", "-p", "chud-desktop", "-p", "chud-cli"])


def samples(dest: Path) -> list[str]:
    """Fichiers d'exemple. Renvoie ce qui a été créé."""
    dest.mkdir(parents=True, exist_ok=True)
    made: list[str] = []
    ff = tool("ffmpeg")
    q = ["-hide_banner", "-loglevel", "error", "-y"]

    def ffmpeg(name: str, args: list[str]) -> None:
        if ff and not (dest / name).exists():
            run([ff, *q, *args, str(dest / name)])
        if (dest / name).exists():
            made.append(name)

    ass = dest / "_sous-titres.ass"
    ass.write_text(
        "[Script Info]\nScriptType: v4.00+\nPlayResX: 1280\nPlayResY: 720\n\n[V4+ Styles]\n"
        "Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, "
        "Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, "
        "MarginR, MarginV, Encoding\n"
        "Style: Default,Arial,48,&H0000C8F9,&H000000FF,&H00280111,&H00000000,1,0,0,0,100,100,0,0,1,3,0,2,20,20,40,1\n\n"
        "[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n"
        "Dialogue: 0,0:00:00.50,0:00:03.00,Default,,0,0,0,,{\\i1}Bonjour la plage !{\\i0}\n"
        "Dialogue: 0,0:00:03.20,0:00:05.80,Default,,0,0,0,,{\\pos(640,120)}Dernier jour de vacances\n",
        encoding="utf-8",
    )
    ffmpeg("vacances.mkv", [
        "-f", "lavfi", "-i", "testsrc2=size=1280x720:rate=30:duration=8",
        "-f", "lavfi", "-i", "sine=frequency=440:duration=8", "-i", str(ass),
        "-map", "0", "-map", "1", "-map", "2", "-c:v", "libx264", "-preset", "veryfast", "-pix_fmt", "yuv420p",
        "-c:a", "aac", "-c:s", "ass", "-metadata:s:a:0", "language=fra", "-metadata:s:s:0", "language=fra",
    ])
    ass.unlink(missing_ok=True)
    # Assez longue pour voir la progression et tester l'annulation pendant un réencodage.
    ffmpeg("film-long.mp4", [
        "-f", "lavfi", "-i", "testsrc2=size=1280x720:rate=30:duration=90",
        "-f", "lavfi", "-i", "sine=frequency=330:duration=90",
        "-c:v", "libx264", "-preset", "veryfast", "-crf", "30", "-pix_fmt", "yuv420p", "-c:a", "aac",
    ])
    ffmpeg("musique.wav", ["-f", "lavfi", "-i", "sine=frequency=523:duration=12", "-ac", "2", "-ar", "48000"])
    # Trois minutes à 44,1 kHz avec balises : FLAC → AAC ou AMR échouaient à la vérification.
    ffmpeg("album.flac", [
        "-f", "lavfi", "-i", "sine=frequency=440:duration=187:sample_rate=44100", "-ac", "2", "-c:a", "flac",
        "-metadata", "title=Chanson d'essai", "-metadata", "artist=Chud", "-metadata", "album=Exemples",
    ])
    # Ogg Vorbis : les balises sont rangées dans le flux, elles étaient perdues.
    ffmpeg("chanson.ogg", [
        "-f", "lavfi", "-i", "sine=frequency=660:duration=20", "-c:a", "libvorbis",
        "-metadata:s:a:0", "title=Été", "-metadata:s:a:0", "artist=Chud",
    ])
    if (dest / "album.flac").exists():
        data = (dest / "album.flac").read_bytes()
        (dest / "coupee.flac").write_bytes(data[: len(data) // 2])
        made.append("coupee.flac")
    (dest / "pas-une-video.mp4").write_bytes(bytes(range(256)) * 64)
    made.append("pas-une-video.mp4")
    ffmpeg("photo.png", ["-f", "lavfi", "-i", "testsrc2=size=1600x1200", "-frames:v", "1"])
    if (dest / "photo.png").exists():
        shutil.copyfile(dest / "photo.png", dest / "faux.jpg")
        made.append("faux.jpg")
    if (dest / "vacances.mkv").exists():
        data = (dest / "vacances.mkv").read_bytes()
        (dest / "abimee.mkv").write_bytes(data[: len(data) // 3])
        made.append("abimee.mkv")

    (dest / "donnees.json").write_text(
        json.dumps([{"titre": "Vacances", "annee": 2026, "note": 4.5, "genres": ["comédie", "été"]},
                    {"titre": "Retour", "annee": 2025, "note": 3, "genres": ["drame"]}],
                   ensure_ascii=False, indent=2),
        encoding="utf-8",
    )
    made.append("donnees.json")
    folder = dest / "Dossier à compresser"
    (folder / "sous-dossier").mkdir(parents=True, exist_ok=True)
    (folder / "lisez-moi.txt").write_text("Un fichier texte.\n", encoding="utf-8")
    (folder / "sous-dossier" / "notes é 文件.txt").write_text("Nom avec accents et caractères chinois.\n", encoding="utf-8")
    made.append("Dossier à compresser/")
    return made


def main() -> None:
    if "--skip-build" not in sys.argv:
        build()
    OUT.mkdir(parents=True, exist_ok=True)
    release = ROOT / "target" / "release"
    for name in (f"chud-converter{EXE}", f"chud{EXE}"):
        shutil.copy2(release / name, OUT / name)
    (OUT / "portable").write_text("Mode portable : réglages et cache dans le dossier data/ ci-contre.\n", encoding="utf-8")
    shutil.copyfile(ROOT / "docs" / "TESTER.md", OUT / "TESTER.md")
    made = samples(OUT / "Exemples")
    print(f"\nPrêt : {OUT}")
    print("Exemples :", ", ".join(made) if made else "aucun (FFmpeg absent)")


if __name__ == "__main__":
    main()
