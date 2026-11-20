#!/usr/bin/env python3
"""Construit un dossier portable prêt à tester : portable/Chud Converter/.

- compile l'interface (npm) puis `chud-converter` et `chud` en version release ;
- crée le fichier `portable` : réglages et cache restent dans `data/` à côté des programmes ;
- génère des fichiers d'exemple réels avec FFmpeg (s'il est installé), et des documents
  (Markdown, CSV, PDF, EPUB) sans aucun outil ;
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
    documents(dest, made)
    return made


def simple_pdf(pages: list[str]) -> bytes:
    """PDF minimal et valide (Helvetica, une ligne de texte par page), sans dépendance."""
    objs = ["<< /Type /Catalog /Pages 2 0 R >>", "", "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"]
    kids = []
    for text in pages:
        page_id, content_id = len(objs) + 1, len(objs) + 2
        kids.append(f"{page_id} 0 R")
        safe = text.replace("\\", "\\\\").replace("(", "\\(").replace(")", "\\)")
        stream = f"BT /F1 28 Tf 72 700 Td ({safe}) Tj ET"
        objs.append(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
            f"/Resources << /Font << /F1 3 0 R >> >> /Contents {content_id} 0 R >>"
        )
        objs.append(f"<< /Length {len(stream)} >>\nstream\n{stream}\nendstream")
    objs[1] = f"<< /Type /Pages /Kids [{' '.join(kids)}] /Count {len(pages)} >>"
    out = bytearray(b"%PDF-1.4\n")
    offsets = []
    for n, body in enumerate(objs, 1):
        offsets.append(len(out))
        out += f"{n} 0 obj\n{body}\nendobj\n".encode("ascii")
    xref = len(out)
    out += f"xref\n0 {len(objs) + 1}\n0000000000 65535 f \n".encode()
    for o in offsets:
        out += f"{o:010d} 00000 n \n".encode()
    out += f"trailer\n<< /Size {len(objs) + 1} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode()
    return bytes(out)


def simple_epub(path: Path) -> None:
    """Petit EPUB 3 valide : deux chapitres et un sommaire."""
    import zipfile

    chapter = (
        '<?xml version="1.0" encoding="utf-8"?>\n'
        '<html xmlns="http://www.w3.org/1999/xhtml" xml:lang="fr"><head><title>{t}</title></head>\n'
        "<body><h1>{t}</h1><p>{p}</p></body></html>"
    )
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr(zipfile.ZipInfo("mimetype"), "application/epub+zip", compress_type=zipfile.ZIP_STORED)
        z.writestr(
            "META-INF/container.xml",
            '<?xml version="1.0"?>\n'
            '<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">'
            '<rootfiles><rootfile full-path="OEBPS/livre.opf" media-type="application/oebps-package+xml"/>'
            "</rootfiles></container>",
        )
        z.writestr(
            "OEBPS/livre.opf",
            '<?xml version="1.0" encoding="utf-8"?>\n'
            '<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id">\n'
            '<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">\n'
            '<dc:identifier id="id">urn:uuid:6f1c2a4e-0b7d-4c8e-9a51-2d3e4f5a6b7c</dc:identifier>\n'
            "<dc:title>Le petit livre d'essai</dc:title><dc:creator>Chud</dc:creator><dc:language>fr</dc:language>\n"
            '<meta property="dcterms:modified">2026-11-19T00:00:00Z</meta></metadata>\n'
            '<manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>\n'
            '<item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/>\n'
            '<item id="c2" href="c2.xhtml" media-type="application/xhtml+xml"/></manifest>\n'
            '<spine><itemref idref="c1"/><itemref idref="c2"/></spine></package>',
        )
        z.writestr(
            "OEBPS/nav.xhtml",
            '<?xml version="1.0" encoding="utf-8"?>\n'
            '<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">'
            "<head><title>Sommaire</title></head><body><nav epub:type=\"toc\"><ol>"
            '<li><a href="c1.xhtml">Premier chapitre</a></li><li><a href="c2.xhtml">Second chapitre</a></li>'
            "</ol></nav></body></html>",
        )
        z.writestr("OEBPS/c1.xhtml", chapter.format(t="Premier chapitre", p="L'été arrive, les élèves partent en vacances."))
        z.writestr("OEBPS/c2.xhtml", chapter.format(t="Second chapitre", p="Ils reviennent à la rentrée, bronzés et reposés."))


def png(width: int, height: int, pixel) -> bytes:
    """PNG RGBA écrit sans aucune bibliothèque ; `pixel(x, y)` renvoie (r, g, b, a)."""
    import struct
    import zlib

    def chunk(kind: bytes, data: bytes) -> bytes:
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

    rows = b"".join(b"\0" + bytes(c for x in range(width) for c in pixel(x, y)) for y in range(height))
    ihdr = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(rows, 9)) + chunk(b"IEND", b"")


def images(dest: Path, made: list[str]) -> None:
    """Exemples d'images de la 0.3.0 (icône, TGA, PCX, BD), sans aucun outil."""
    import struct
    import zipfile

    def disc(x: int, y: int) -> tuple[int, int, int, int]:
        inside = (x - 512) ** 2 + (y - 256) ** 2 < 200**2
        return (230, 120, 30, 255) if inside else (40, 90, 200, 0)

    (dest / "grande-image.png").write_bytes(png(1024, 512, disc))
    w, h = 64, 48
    pixels = [((x * 4) % 256, (y * 5) % 256, 128) for y in range(h) for x in range(w)]
    # TGA en couleurs vraies sans palette : commence par 00 00 02 00, comme un curseur Windows.
    tga = struct.pack("<BBBHHBHHHHBB", 0, 0, 2, 0, 0, 0, 0, 0, w, h, 24, 0x20)
    tga += bytes(c for (r, g, b) in pixels for c in (b, g, r))
    (dest / "image.tga").write_bytes(tga)
    # PCX 24 bits : trois plans par ligne, compression RLE (octets ≥ 0xC0 précédés d'un compte).
    header = struct.pack("<BBBBHHHHHH", 10, 5, 1, 8, 0, 0, w - 1, h - 1, 72, 72) + bytes(48)
    header += struct.pack("<BBHHHH", 0, 3, w, 1, 0, 0) + bytes(54)
    body = bytearray()
    for y in range(h):
        for plane in range(3):
            for x in range(w):
                v = pixels[y * w + x][plane]
                body += bytes([0xC1, v]) if v >= 0xC0 else bytes([v])
    (dest / "image.pcx").write_bytes(header + bytes(body))
    with zipfile.ZipFile(dest / "bd.cbz", "w") as z:
        for i, color in enumerate([(200, 60, 60), (60, 160, 90)], start=1):
            z.writestr(f"page{i:02}.png", png(120, 160, lambda x, y, c=color: (*c, 255)))
    made += ["grande-image.png", "image.tga", "image.pcx", "bd.cbz"]


def documents(dest: Path, made: list[str]) -> None:
    """Exemples pour Pandoc, LibreOffice, Calibre, qpdf, Ghostscript et Tesseract, sans aucun outil."""
    (dest / "notes.md").write_text(
        "# Notes de réunion\n\nOrdre du jour :\n\n1. **Budget** de l'été\n2. *Calendrier*\n\n"
        "| Poste | Montant |\n|---|---|\n| Matériel | 1 200 € |\n| Voyage | 800 € |\n\n"
        "> Prochaine réunion : jeudi.\n",
        encoding="utf-8",
    )
    (dest / "tableau.csv").write_text("Poste,Montant,Payé\nMatériel,1200,oui\nVoyage,800,non\nRepas,350,oui\n", encoding="utf-8")
    (dest / "document.pdf").write_bytes(
        simple_pdf(["Chud Converter : page 1", "Deuxieme page du document", "Troisieme et derniere page"])
    )
    simple_epub(dest / "livre.epub")
    made += ["notes.md", "tableau.csv", "document.pdf", "livre.epub"]
    # Image liée qui n'existe pas à côté : Pandoc ne lit jamais d'autre fichier que la source.
    (dest / "notes-image.md").write_text(
        "# Carnet de voyage\n\nUne photo du départ :\n\n![Le départ](couverture.jpg)\n\nFin du carnet.\n", encoding="utf-8"
    )
    made.append("notes-image.md")
    # ODT avec une image, écrit par LibreOffice lui-même (CHUD_SOFFICE ou soffice du PATH).
    soffice = os.environ.get("CHUD_SOFFICE") or tool("soffice")
    if soffice:
        import base64
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            pic = base64.b64encode(png(80, 60, lambda x, y: (x * 3, y * 4, 150, 255))).decode()
            html = Path(tmp) / "lettre.html"
            html.write_text(
                f"<html><body><h1>Lettre</h1><p>Bonjour, voici une image :</p><img src='data:image/png;base64,{pic}'/>"
                "<p>Cordialement.</p></body></html>",
                encoding="utf-8",
            )
            run([soffice, "--headless", f"-env:UserInstallation=file:///{tmp.replace(os.sep, '/')}/profil",
                 "--convert-to", "odt:writer8", "--outdir", str(dest), str(html)])
        if (dest / "lettre.odt").exists():
            made.append("lettre.odt")


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
    images(OUT / "Exemples", made)
    print(f"\nPrêt : {OUT}")
    print("Exemples :", ", ".join(made) if made else "aucun (FFmpeg absent)")


if __name__ == "__main__":
    main()
