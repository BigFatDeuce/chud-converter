//! Tests réels des adaptateurs Pandoc, LibreOffice, Calibre, qpdf, Ghostscript, Tesseract,
//! ImageMagick, libvips, MKVToolNix, MediaInfo et ExifTool.
//!
//! Chaque outil est cherché dans `CHUD_TOOL_<ID>` (chemin de l'exécutable) puis dans le `PATH`.
//! Absent : test ignoré (« SKIP »), sauf si `CHUD_REQUIRE_TOOLS` est défini (CI Linux, où
//! tous sont installés), où c'est un échec. Les fichiers portent des noms piégés (`é [1] %d`)
//! pour prouver qu'aucun outil ne les interprète.

use chud_core::{Engine, EngineOrigin, ExecContext, Feature, FormatId, OptionValues, StepTask, Verification};
use std::path::{Path, PathBuf};

fn tool(id: &str, names: &[&str]) -> Option<PathBuf> {
    let var = format!("CHUD_TOOL_{}", id.to_uppercase());
    let found = std::env::var_os(&var).map(PathBuf::from).filter(|p| p.is_file()).or_else(|| {
        let dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
        names
            .iter()
            .flat_map(|n| {
                let n = if cfg!(windows) && !n.contains('.') { format!("{n}.exe") } else { (*n).to_owned() };
                dirs.iter().map(move |d| d.join(&n)).collect::<Vec<_>>()
            })
            .find(|p| p.is_file())
    });
    if found.is_none() {
        assert!(std::env::var_os("CHUD_REQUIRE_TOOLS").is_none(), "{id} introuvable alors que CHUD_REQUIRE_TOOLS est défini");
        eprintln!("SKIP : {id} introuvable ({var} ou PATH)");
    }
    found
}

fn workdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("chud-tools-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Exécute une étape complète (options par défaut + `opts`) et renvoie la vérification.
fn convert(e: &dyn Engine, input: &Path, from: &str, to: &str, out: &Path, opts: &[(&str, chud_core::OptionValue)]) -> Verification {
    let (from, to) = (FormatId::from(from), FormatId::from(to));
    assert!(e.conversions().iter().any(|c| c.from == from && c.to == to), "{} ne déclare pas {from} → {to}", e.id());
    let mut values = OptionValues::new();
    for (k, v) in opts {
        values.set(k, v.clone());
    }
    let options = values.resolve(&e.options(&from, &to)).unwrap();
    let task = StepTask { input: input.into(), output: out.into(), from, to, options, source: None };
    let ctx = ExecContext::new(out.parent().unwrap().join("work"));
    let r = e.execute(&task, &ctx).unwrap_or_else(|err| panic!("{} : {err}", e.id()));
    eprintln!("{} {} → {} : {:?}", e.id(), task.from, task.to, r.notes);
    let v = e.verify(out, &task, &ctx);
    assert!(v.is_ok(), "{} : vérification {v:?}", e.id());
    let left: Vec<_> = std::fs::read_dir(ctx.work_dir).map(|d| d.flatten().collect()).unwrap_or_default();
    assert!(left.is_empty(), "dossier de travail nettoyé");
    v
}

const PS: &str = "%!PS-Adobe-3.0\n/Helvetica findfont 48 scalefont setfont\n72 600 moveto (Bonjour Chud) show\nshowpage\n/Helvetica findfont 48 scalefont setfont\n72 600 moveto (Page deux) show\nshowpage\n";

#[test]
fn pandoc_documents() {
    let Some(exe) = tool("pandoc", &["pandoc"]) else { return };
    let e = chud_engines::pandoc::Pandoc::new(exe, None, EngineOrigin::System).unwrap();
    let d = workdir("pandoc");
    let md = d.join("é [1] %d.md");
    std::fs::write(&md, "# Titre\n\nUn *paragraphe* avec un tableau :\n\n| a | b |\n|---|---|\n| 1 | 2 |\n").unwrap();
    let docx = d.join("out é.docx");
    assert!(matches!(convert(&e, &md, "md", "docx", &docx, &[]), Verification::Decoded(_)));
    let html = d.join("out.html");
    assert!(matches!(convert(&e, &docx, "docx", "html", &html, &[]), Verification::Decoded(_)));
    assert!(std::fs::read_to_string(&html).unwrap().contains("paragraphe"));
    convert(&e, &docx, "docx", "txt", &d.join("out.txt"), &[]);
    convert(&e, &md, "md", "epub", &d.join("out.epub"), &[]);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn libreoffice_office_documents() {
    let Some(exe) = tool("libreoffice", if cfg!(windows) { &["soffice.com"] } else { &["soffice", "libreoffice"] }) else { return };
    let e = chud_engines::libreoffice::LibreOffice::new(exe, None, EngineOrigin::System).unwrap();
    let d = workdir("lo");
    let txt = d.join("é [1] %d.txt");
    std::fs::write(&txt, "Bonjour depuis Chud Converter.\nDeuxième ligne.\n").unwrap();
    let docx = d.join("doc.docx");
    convert(&e, &txt, "txt", "docx", &docx, &[]);
    let pdf = d.join("doc.pdf");
    convert(&e, &docx, "docx", "pdf", &pdf, &[]);
    let csv = d.join("table é.csv");
    std::fs::write(&csv, "nom,valeur\nA,1\nB,2\n").unwrap();
    let xlsx = d.join("table.xlsx");
    convert(&e, &csv, "csv", "xlsx", &xlsx, &[]);
    convert(&e, &xlsx, "xlsx", "ods", &d.join("table.ods"), &[]);
    // Deuxième conversion : le profil isolé est réutilisé.
    convert(&e, &docx, "docx", "odt", &d.join("doc.odt"), &[]);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn calibre_ebooks() {
    let Some(exe) = tool("calibre", &["ebook-convert"]) else { return };
    let e = chud_engines::calibre::Calibre::new(exe, None, EngineOrigin::System).unwrap();
    let d = workdir("calibre");
    let html = d.join("é [1] %d.html");
    std::fs::write(&html, "<html><head><title>Livre</title></head><body><h1>Chapitre 1</h1><p>Il était une fois.</p></body></html>")
        .unwrap();
    let epub = d.join("livre.epub");
    convert(&e, &html, "html", "epub", &epub, &[]);
    convert(&e, &epub, "epub", "azw3", &d.join("livre.azw3"), &[]);
    convert(&e, &epub, "epub", "txt", &d.join("livre.txt"), &[]);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn pdf_chain_ghostscript_qpdf_tesseract() {
    let Some(gs) = tool("ghostscript", if cfg!(windows) { &["gswin64c"] } else { &["gs"] }) else { return };
    let g = chud_engines::ghostscript::Ghostscript::new(gs, None, EngineOrigin::System).unwrap();
    let d = workdir("pdf");
    let ps = d.join("é [1] %d.ps");
    std::fs::write(&ps, PS).unwrap();
    let pdf = d.join("doc %d.pdf");
    assert!(matches!(convert(&g, &ps, "ps", "pdf", &pdf, &[]), Verification::Decoded(_)));
    let png = d.join("page2.png");
    convert(&g, &pdf, "pdf", "png", &png, &[("page", chud_core::OptionValue::Int(2)), ("dpi", chud_core::OptionValue::Int(200))]);
    convert(&g, &pdf, "pdf", "tiff", &d.join("pages.tiff"), &[]);
    if let Some(q) = tool("qpdf", &["qpdf"]) {
        let q = chud_engines::qpdf::Qpdf::new(q, None, EngineOrigin::System).unwrap();
        let info = q.probe(&pdf, &FormatId::from("pdf")).unwrap();
        assert!(info.summary.iter().any(|(_, v)| v == "2"), "{:?}", info.summary);
        assert!(matches!(convert(&q, &pdf, "pdf", "pdf", &d.join("web.pdf"), &[]), Verification::Structural(_)));
        let one = d.join("une page.pdf");
        convert(&q, &pdf, "pdf", "pdf", &one, &[("pages", chud_core::OptionValue::Text("2".into()))]);
        assert!(q.probe(&one, &FormatId::from("pdf")).unwrap().summary.iter().any(|(_, v)| v == "1"));
    }
    if let Some(t) = tool("tesseract", &["tesseract"]) {
        let t = chud_engines::tesseract::Tesseract::new(t, None, EngineOrigin::System).unwrap();
        let txt = d.join("ocr.txt");
        convert(&t, &png, "png", "txt", &txt, &[]);
        let text = std::fs::read_to_string(&txt).unwrap();
        assert!(text.to_lowercase().contains("page"), "texte reconnu : {text}");
        convert(&t, &png, "png", "pdf", &d.join("ocr.pdf"), &[]);
    }
    let _ = std::fs::remove_dir_all(&d);
}

fn sample_png(path: &Path) {
    let img = image::RgbaImage::from_fn(64, 48, |x, y| image::Rgba([(x * 4) as u8, (y * 5) as u8, 128, if x < 8 { 0 } else { 255 }]));
    img.save(path).unwrap();
}

#[test]
fn imagemagick_images() {
    let Some(exe) = tool("imagemagick", &["magick"]) else { return };
    let e = chud_engines::imagemagick::ImageMagick::new(exe, None, EngineOrigin::System).unwrap();
    let d = workdir("im");
    let png = d.join("é [1] %d.png");
    sample_png(&png);
    let psd = d.join("img.psd");
    assert!(matches!(convert(&e, &png, "png", "psd", &psd, &[]), Verification::Decoded(_)));
    convert(&e, &psd, "psd", "png", &d.join("back.png"), &[]);
    convert(&e, &png, "png", "jpeg", &d.join("img.jpg"), &[]);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn libvips_images() {
    let Some(exe) = tool("libvips", &["vips"]) else { return };
    let e = chud_engines::libvips::Libvips::new(exe, None, EngineOrigin::System).unwrap();
    let d = workdir("vips");
    let png = d.join("é [1] %d.png");
    sample_png(&png);
    let webp = d.join("img.webp");
    assert!(matches!(convert(&e, &png, "png", "webp", &webp, &[]), Verification::Decoded(_)));
    convert(&e, &webp, "webp", "tiff", &d.join("img.tiff"), &[]);
    let _ = std::fs::remove_dir_all(&d);
}

/// Petite vidéo de test produite par FFmpeg (absent : test ignoré).
fn sample_mp4(dir: &Path) -> Option<PathBuf> {
    let ff = tool("ffmpeg", &["ffmpeg"])?;
    let out = dir.join("é [1] %d.mp4");
    let st = std::process::Command::new(ff)
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=d=1:s=160x120",
            "-f",
            "lavfi",
            "-i",
            "sine=d=1",
            "-shortest",
            "-c:v",
            "mpeg4",
            "-c:a",
            "aac",
        ])
        .arg(&out)
        .status()
        .ok()?;
    st.success().then_some(out)
}

#[test]
fn mkvtoolnix_and_mediainfo() {
    let d = workdir("mkv");
    let Some(mp4) = sample_mp4(&d) else { return };
    if let Some(exe) = tool("mkvtoolnix", &["mkvmerge"]) {
        let e = chud_engines::mkvtoolnix::MkvToolNix::new(exe, None, EngineOrigin::System).unwrap();
        let v = convert(&e, &mp4, "mp4", "mkv", &d.join("out.mkv"), &[]);
        assert!(matches!(&v, Verification::Structural(s) if s.contains('2')), "{v:?}");
    }
    if let Some(exe) = tool("mediainfo", &["mediainfo"]) {
        let e = chud_engines::mediainfo::MediaInfo::new(exe, None, EngineOrigin::System).unwrap();
        let info = e.probe(&mp4, &FormatId::from("mp4")).expect("sonde MediaInfo");
        assert!(info.summary.iter().any(|(_, v)| v.contains("MPEG-4")), "{:?}", info.summary);
    }
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn exiftool_reads_metadata() {
    let Some(exe) = tool("exiftool", &["exiftool"]) else { return };
    let d = workdir("exif");
    let jpg = d.join("é [1] %d.jpg");
    image::RgbImage::from_pixel(16, 16, image::Rgb([200, 10, 10])).save(&jpg).unwrap();
    // Ajout de métadonnées par ExifTool lui-même (fichier d'arguments UTF-8, comme l'adaptateur).
    let args = d.join("args.txt");
    std::fs::write(&args, format!("-overwrite_original\n-EXIF:Artist=Alice\n-XMP-dc:Title=Essai\n{}\n", jpg.display())).unwrap();
    let st = std::process::Command::new(&exe).args(["-charset", "filename=UTF8", "-q", "-@"]).arg(&args).status().unwrap();
    assert!(st.success());
    let e = chud_engines::exiftool::ExifTool::new(exe, None, EngineOrigin::System).unwrap();
    let info = e.probe(&jpg, &FormatId::from("jpeg")).expect("sonde ExifTool");
    let f = info.features.unwrap();
    assert!(f.contains(&Feature::new("exif")) && f.contains(&Feature::new("xmp")), "{f:?}");
    assert!(info.summary.iter().any(|(_, v)| v == "Alice"), "{:?}", info.summary);
    let _ = std::fs::remove_dir_all(&d);
}
