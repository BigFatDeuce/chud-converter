//! Détection du format réel d'un fichier.
//!
//! Ordre : signature binaire (crate `file-format`, ~450 signatures et lecteurs de conteneurs),
//! affinage par la famille déclarée dans le catalogue (`based_on`), heuristiques de contenu
//! pour les formats texte, puis extension seule (confiance faible, toujours signalée).

use crate::format::{FormatCatalog, FormatDef, FormatId};
use file_format::FileFormat;
use serde::Serialize;
use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Low,
    Medium,
    High,
}

impl Confidence {
    pub fn label(self) -> &'static str {
        match self {
            Confidence::Low => crate::tr!("faible", "low"),
            Confidence::Medium => crate::tr!("moyenne", "medium"),
            Confidence::High => crate::tr!("élevée", "high"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectionMethod {
    /// Octets magiques ou structure binaire.
    Signature,
    /// Signature d'un conteneur + extension compatible (CBZ dans un ZIP, NEF dans un TIFF…).
    Container,
    /// Analyse du contenu texte.
    Content,
    /// Extension seule.
    Extension,
    Directory,
    None,
}

impl DetectionMethod {
    pub fn label(self) -> &'static str {
        match self {
            DetectionMethod::Signature => crate::tr!("signature binaire", "binary signature"),
            DetectionMethod::Container => crate::tr!("conteneur et extension", "container and extension"),
            DetectionMethod::Content => crate::tr!("analyse du contenu", "content analysis"),
            DetectionMethod::Extension => crate::tr!("extension seule", "extension only"),
            DetectionMethod::Directory => crate::tr!("dossier", "folder"),
            DetectionMethod::None => crate::tr!("aucune", "none"),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct SignatureInfo {
    pub name: String,
    pub media_type: String,
    pub extension: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Detection {
    /// Format reconnu dans le catalogue, s'il y en a un.
    pub format: Option<FormatId>,
    pub confidence: Confidence,
    pub method: DetectionMethod,
    /// Ce que dit la signature binaire, même si le format n'est pas dans le catalogue.
    pub signature: Option<SignatureInfo>,
    /// Format suggéré par l'extension du nom de fichier.
    pub by_extension: Option<FormatId>,
    /// L'extension ne correspond pas au contenu réel.
    pub extension_mismatch: bool,
    pub size: u64,
    /// Encodage texte détecté (« utf-8 », « utf-16le »…), pour les fichiers texte.
    pub text_encoding: Option<String>,
    /// Explication lisible (en français) lorsque la détection est incertaine ou échoue.
    pub note: Option<String>,
}

impl Detection {
    fn new(size: u64, by_extension: Option<FormatId>) -> Self {
        Self {
            format: None,
            confidence: Confidence::Low,
            method: DetectionMethod::None,
            signature: None,
            by_extension,
            extension_mismatch: false,
            size,
            text_encoding: None,
            note: None,
        }
    }
}

/// Taille de l'échantillon lu pour les heuristiques texte.
const TEXT_SAMPLE: usize = 64 * 1024;

pub fn detect_path(path: &Path, catalog: &FormatCatalog) -> io::Result<Detection> {
    let meta = std::fs::metadata(path)?;
    if meta.is_dir() {
        let mut d = Detection::new(0, None);
        d.format = Some(FormatId::new("directory"));
        d.confidence = Confidence::High;
        d.method = DetectionMethod::Directory;
        return Ok(d);
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
    let reader = BufReader::new(File::open(path)?);
    detect_reader(reader, name.as_deref(), meta.len(), catalog)
}

pub fn detect_bytes(bytes: &[u8], file_name: Option<&str>, catalog: &FormatCatalog) -> Detection {
    detect_reader(io::Cursor::new(bytes), file_name, bytes.len() as u64, catalog).expect("la lecture en mémoire ne peut pas échouer")
}

pub fn detect_reader<R: Read + Seek>(mut reader: R, file_name: Option<&str>, size: u64, catalog: &FormatCatalog) -> io::Result<Detection> {
    let ext_def = file_name.and_then(|n| catalog.guess_from_name(n));
    let mut d = Detection::new(size, ext_def.map(|f| f.id.clone()));
    if size == 0 {
        d.note = Some("fichier vide".into());
        return Ok(d);
    }

    let mut ff = FileFormat::from_reader(&mut reader)?;
    reader.seek(SeekFrom::Start(0))?;
    // Les 4 octets « 00 00 02 00 » d'un curseur sont aussi le début de tout TGA en couleurs
    // vraies sans palette (00 00 01 00, icône, reste possible pour un TGA mal formé) : un vrai
    // curseur ou une vraie icône annonce au moins une image (octets 4 et 5).
    if matches!(ff, FileFormat::WindowsCursor | FileFormat::WindowsIcon) {
        let mut head = [0u8; 6];
        let count = reader.read_exact(&mut head).map(|_| u16::from_le_bytes([head[4], head[5]])).unwrap_or(0);
        if count == 0 {
            ff = FileFormat::ArbitraryBinaryData;
        }
        reader.seek(SeekFrom::Start(0))?;
    }

    match ff {
        FileFormat::PlainText | FileFormat::ArbitraryBinaryData | FileFormat::Empty => {
            let mut sample = Vec::with_capacity(TEXT_SAMPLE.min(size as usize));
            (&mut reader).take(TEXT_SAMPLE as u64).read_to_end(&mut sample)?;
            let complete = sample.len() as u64 >= size;
            match decode_text(&sample) {
                Some((text, encoding)) if ff == FileFormat::PlainText || looks_textual(&text) => {
                    d.text_encoding = Some(encoding.to_string());
                    classify_text(&mut d, &text, complete, ext_def, catalog);
                }
                _ => {
                    if let Some(e) = ext_def.filter(|e| e.id == "tga" && plausible_tga(&sample)) {
                        // Le TGA n'a pas de signature : un en-tête cohérent est le mieux possible.
                        d.format = Some(e.id.clone());
                        d.confidence = Confidence::Medium;
                        d.method = DetectionMethod::Extension;
                        d.note = Some(
                            crate::tr!(
                                "en-tête Targa cohérent (ce format n'a pas de signature)",
                                "consistent Targa header (this format has no signature)"
                            )
                            .into(),
                        );
                    } else if let Some(e) = ext_def {
                        d.format = Some(e.id.clone());
                        d.confidence = Confidence::Low;
                        d.method = DetectionMethod::Extension;
                        d.note = Some(
                            crate::tr!(
                                "signature binaire non reconnue : format déduit de l'extension uniquement",
                                "unrecognized binary signature: format inferred from the extension only"
                            )
                            .into(),
                        );
                    } else {
                        d.note = Some(
                            crate::tr!(
                                "format non reconnu (ni signature connue, ni extension connue)",
                                "unrecognized format (neither a known signature nor a known extension)"
                            )
                            .into(),
                        );
                    }
                }
            }
        }
        ff => {
            let sig = SignatureInfo {
                name: ff.name().to_string(),
                media_type: ff.media_type().to_string(),
                extension: ff.extension().to_string(),
            };
            let mut sig_def = map_signature(ff, catalog);
            // AC-3 et E-AC-3 ont la même signature (0B 77) : le champ « bsid » (6e octet) les
            // distingue — 0 à 10 pour l'AC-3, 11 à 16 pour l'E-AC-3.
            if sig_def.is_some_and(|s| s.id == "ac3") {
                let mut head = [0u8; 6];
                if reader.read_exact(&mut head).is_ok() && head[..2] == [0x0B, 0x77] && head[5] >> 3 > 10 {
                    sig_def = catalog.get("eac3").or(sig_def);
                }
                reader.seek(SeekFrom::Start(0))?;
            }
            d.signature = Some(sig.clone());
            match sig_def {
                Some(s) => apply_signature(&mut d, s, ext_def, catalog),
                None => {
                    // Format reconnu par sa signature mais absent du catalogue.
                    let same_family = ext_def.is_some_and(|e| kind_matches(ff, e));
                    if let (true, Some(e)) = (same_family, ext_def) {
                        d.format = Some(e.id.clone());
                        d.confidence = Confidence::Medium;
                        d.method = DetectionMethod::Extension;
                        d.note = Some(crate::trf!(
                            "signature « {} » ; format retenu d'après l'extension (même famille)",
                            "signature “{}”; format chosen from the extension (same family)",
                            sig.name
                        ));
                    } else {
                        d.extension_mismatch = ext_def.is_some();
                        d.note = Some(crate::trf!(
                            "reconnu comme « {} », format non pris en charge",
                            "recognized as “{}”, an unsupported format",
                            sig.name
                        ));
                    }
                }
            }
        }
    }
    Ok(d)
}

/// En-tête Targa vraisemblable : type d'image, palette, profondeur et dimensions cohérents.
fn plausible_tga(h: &[u8]) -> bool {
    if h.len() < 18 {
        return false;
    }
    let u16_at = |i: usize| u16::from_le_bytes([h[i], h[i + 1]]);
    let palette_ok = match h[1] {
        0 => u16_at(5) == 0,
        1 => u16_at(5) > 0 && matches!(h[7], 15 | 16 | 24 | 32),
        _ => false,
    };
    palette_ok && matches!(h[2], 1 | 2 | 3 | 9 | 10 | 11) && matches!(h[16], 8 | 15 | 16 | 24 | 32) && u16_at(12) > 0 && u16_at(14) > 0
}

fn apply_signature(d: &mut Detection, sig: &FormatDef, ext: Option<&FormatDef>, catalog: &FormatCatalog) {
    d.confidence = Confidence::High;
    match ext {
        // L'extension désigne un format porté par la signature (CBZ → ZIP) : on la retient.
        Some(e) if e.id != sig.id && e.based_on.contains(&sig.id) => {
            d.format = Some(e.id.clone());
            d.method = DetectionMethod::Container;
        }
        Some(e) => {
            d.format = Some(sig.id.clone());
            d.method = DetectionMethod::Signature;
            d.extension_mismatch = !catalog.related(e.id.as_str(), sig.id.as_str());
        }
        None => {
            d.format = Some(sig.id.clone());
            d.method = DetectionMethod::Signature;
        }
    }
}

/// Associe un résultat de `file-format` à une entrée du catalogue.
fn map_signature(ff: FileFormat, catalog: &FormatCatalog) -> Option<&FormatDef> {
    let ext = ff.extension();
    // Correspondances explicites quand l'extension de `file-format` diffère du catalogue.
    let forced = match ff.short_name().unwrap_or("") {
        "CFB" => Some("cfb"),
        _ => None,
    };
    if let Some(id) = forced {
        return catalog.get(id);
    }
    catalog.by_extension(ext).into_iter().find(|f| !f.is_variant()).or_else(|| catalog.by_mime(ff.media_type()))
}

fn kind_matches(ff: FileFormat, def: &FormatDef) -> bool {
    use crate::format::Category as C;
    use file_format::Kind as K;
    matches!(
        (ff.kind(), def.category),
        (K::Audio, C::Audio)
            | (K::Video, C::Video)
            | (K::Image, C::Image)
            | (K::Archive | K::Compressed | K::Disk | K::Package, C::Archive)
            | (K::Document, C::Document)
            | (K::Ebook, C::Ebook)
            | (K::Font, C::Font)
            | (K::Model, C::Model3d)
            | (K::Geospatial, C::Geo)
            | (K::Spreadsheet, C::Spreadsheet)
            | (K::Presentation, C::Presentation)
            | (K::Subtitle, C::Subtitle)
    )
}

/// Décode un échantillon en texte. Renvoie `None` si ce n'est manifestement pas du texte.
fn decode_text(sample: &[u8]) -> Option<(String, &'static str)> {
    if let Some(rest) = sample.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return Some((String::from_utf8_lossy(rest).into_owned(), "utf-8-bom"));
    }
    if let Some(rest) = sample.strip_prefix(&[0xFF, 0xFE]) {
        let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        return Some((String::from_utf16_lossy(&units), "utf-16le"));
    }
    if let Some(rest) = sample.strip_prefix(&[0xFE, 0xFF]) {
        let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        return Some((String::from_utf16_lossy(&units), "utf-16be"));
    }
    if sample.contains(&0) {
        return None;
    }
    match std::str::from_utf8(sample) {
        Ok(s) => Some((s.to_owned(), "utf-8")),
        // Échantillon coupé au milieu d'un caractère multi-octets.
        Err(e) if e.error_len().is_none() => Some((String::from_utf8_lossy(&sample[..e.valid_up_to()]).into_owned(), "utf-8")),
        Err(_) => Some((sample.iter().map(|&b| b as char).collect(), "8-bit")),
    }
}

fn looks_textual(text: &str) -> bool {
    let total = text.chars().count().max(1);
    let control = text.chars().filter(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t' | '\x0c')).count();
    control * 100 / total < 2
}

fn classify_text(d: &mut Detection, text: &str, complete: bool, ext: Option<&FormatDef>, catalog: &FormatCatalog) {
    let guess = sniff_text_format(text, complete);
    match (ext.filter(|e| e.text), guess) {
        // L'extension désigne un format texte : on la suit, le contenu sert de contrôle.
        (Some(e), g) => {
            d.format = Some(e.id.clone());
            d.method = DetectionMethod::Extension;
            d.confidence = Confidence::Medium;
            if let Some(g) = g {
                if catalog.related(g, e.id.as_str()) || g == e.id.as_str() {
                    d.method = DetectionMethod::Content;
                    d.confidence = Confidence::High;
                } else if !(g == "xml" && is_xml_family(e)) && !(g == "json" && is_json_family(e)) {
                    d.note = Some(crate::trf!("le contenu ressemble plutôt à du {g}", "the content looks more like {g}"));
                }
            }
        }
        (None, Some(g)) => {
            d.format = Some(FormatId::new(g));
            d.method = DetectionMethod::Content;
            d.confidence = Confidence::Medium;
            d.extension_mismatch = ext.is_some();
        }
        (None, None) => {
            d.format = Some(FormatId::new("txt"));
            d.method = DetectionMethod::Content;
            d.confidence = Confidence::Medium;
            d.extension_mismatch = ext.is_some_and(|e| e.id != "txt");
        }
    }
}

fn is_xml_family(f: &FormatDef) -> bool {
    f.based_on.iter().any(|b| b == "xml") || f.id == "html"
}

fn is_json_family(f: &FormatDef) -> bool {
    f.based_on.iter().any(|b| b == "json") || f.id == "ndjson"
}

/// Heuristiques de contenu pour les formats texte. Renvoie un identifiant du catalogue.
pub fn sniff_text_format(text: &str, complete: bool) -> Option<&'static str> {
    let t = text.trim_start_matches('\u{feff}').trim_start();
    let lower_head: String = t.chars().take(512).collect::<String>().to_lowercase();

    if t.starts_with("{\\rtf") {
        return Some("rtf");
    }
    if t.starts_with("WEBVTT") {
        return Some("vtt");
    }
    if t.starts_with("[Script Info]") {
        return Some("ass");
    }
    if t.starts_with('<') {
        return Some(if lower_head.contains("<svg") {
            "svg"
        } else if lower_head.starts_with("<!doctype html") || lower_head.contains("<html") {
            "html"
        } else if lower_head.contains("<kml") {
            "kml"
        } else if lower_head.contains("<gpx") {
            "gpx"
        } else if lower_head.contains("<fictionbook") {
            "fb2"
        } else if lower_head.contains("<collada") {
            "dae"
        } else if lower_head.contains("<tt ") || lower_head.contains("<tt>") {
            "ttml"
        } else {
            "xml"
        });
    }
    if t.starts_with('{') || t.starts_with('[') {
        let mut lines = t.lines().map(str::trim).filter(|l| !l.is_empty());
        let first = lines.next().unwrap_or("");
        let second = lines.next();
        if complete && serde_json::from_str::<serde_json::Value>(t).is_ok() {
            return Some("json");
        }
        if serde_json::from_str::<serde_json::Value>(first).is_ok()
            && second.is_some_and(|s| serde_json::from_str::<serde_json::Value>(s).is_ok())
        {
            return Some("ndjson");
        }
        if !complete {
            return Some("json");
        }
        if t.starts_with('[') && !t.contains('\n') {
            return None;
        }
    }
    let lines: Vec<&str> = t.lines().take(20).collect();
    // SubRip : « 1 » puis « 00:00:01,000 --> 00:00:02,000 ».
    if lines.len() >= 2 && lines[0].trim().chars().all(|c| c.is_ascii_digit()) && !lines[0].trim().is_empty() && lines[1].contains("-->") {
        return Some("srt");
    }
    if lines.first().is_some_and(|l| is_microdvd(l)) {
        return Some("microdvd");
    }
    if lines.iter().filter(|l| is_lrc(l)).count() >= 2 {
        return Some("lrc");
    }
    if t.starts_with("%YAML") || t.starts_with("---\n") || t.starts_with("---\r\n") {
        return Some("yaml");
    }
    if let Some(delim) = sniff_delimiter(&lines) {
        return Some(if delim == b'\t' { "tsv" } else { "csv" });
    }
    None
}

fn is_microdvd(l: &str) -> bool {
    let mut parts = l.trim().splitn(3, '}');
    let a = parts.next().unwrap_or("");
    let b = parts.next().unwrap_or("");
    a.starts_with('{')
        && b.starts_with('{')
        && a[1..].chars().all(|c| c.is_ascii_digit())
        && b[1..].chars().all(|c| c.is_ascii_digit())
        && a.len() > 1
}

fn is_lrc(l: &str) -> bool {
    let l = l.trim();
    l.len() >= 10 && l.starts_with('[') && l.as_bytes().get(3) == Some(&b':') && l[1..3].chars().all(|c| c.is_ascii_digit())
}

/// Détecte un séparateur CSV cohérent sur les premières lignes (`,`, `;`, tabulation, `|`).
pub fn sniff_delimiter(lines: &[&str]) -> Option<u8> {
    let lines: Vec<&str> = lines.iter().copied().filter(|l| !l.trim().is_empty()).take(10).collect();
    if lines.len() < 2 {
        return None;
    }
    let mut best: Option<(u8, usize)> = None;
    for delim in *b"\t;,|" {
        let counts: Vec<usize> = lines.iter().map(|l| count_outside_quotes(l, delim)).collect();
        let first = counts[0];
        // Tolérance : la dernière ligne de l'échantillon peut être tronquée.
        let consistent = counts[..counts.len() - 1].iter().all(|&c| c == first) && first > 0;
        if consistent && best.is_none_or(|(_, n)| first > n) {
            best = Some((delim, first));
        }
    }
    best.map(|(d, _)| d)
}

fn count_outside_quotes(line: &str, delim: u8) -> usize {
    let mut in_quotes = false;
    let mut n = 0;
    for b in line.bytes() {
        if b == b'"' {
            in_quotes = !in_quotes;
        } else if b == delim && !in_quotes {
            n += 1;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn cat() -> &'static FormatCatalog {
        FormatCatalog::builtin()
    }

    const PNG_1X1: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00,
        0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63,
        0xF8, 0xCF, 0xC0, 0xF0, 0x1F, 0x00, 0x05, 0x00, 0x01, 0xFF, 0x89, 0x99, 0x3D, 0x1D, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44,
        0xAE, 0x42, 0x60, 0x82,
    ];

    /// Débuts de vrais fichiers produits par FFmpeg : même signature 0B 77, « bsid » 8 et 16.
    #[test]
    fn ac3_and_eac3_are_told_apart() {
        let ac3 = [
            0x0B, 0x77, 0x87, 0x58, 0x54, 0x40, 0x43, 0xE1, 0x06, 0xF5, 0x75, 0xF1, 0x25, 0x26, 0x19, 0x99, 0xFF, 0xF7, 0xAE, 0xF9, 0xF3,
            0xE7, 0xCF, 0x9F, 0xB0, 0xEF, 0x70, 0xDF, 0xBE, 0x86, 0xF9, 0xF5,
        ];
        let eac3 = [
            0x0B, 0x77, 0x01, 0xA0, 0x74, 0x87, 0xC0, 0x00, 0x20, 0x84, 0x21, 0x08, 0x32, 0x80, 0xAE, 0x09, 0x29, 0x21, 0x99, 0x9F, 0xBD,
            0x77, 0xCF, 0x9F, 0x3E, 0x7C, 0xFD, 0x87, 0x7B, 0x86, 0xFD, 0xF4,
        ];
        let a = detect_bytes(&ac3, Some("a.ac3"), cat());
        assert_eq!(a.format.as_ref().map(|f| f.as_str()), Some("ac3"));
        assert!(!a.extension_mismatch);
        let e = detect_bytes(&eac3, Some("a.eac3"), cat());
        assert_eq!(e.format.as_ref().map(|f| f.as_str()), Some("eac3"));
        assert!(!e.extension_mismatch, "{e:?}");
        // Le contenu décide, pas l'extension.
        let e = detect_bytes(&eac3, Some("faux.ac3"), cat());
        assert_eq!(e.format.as_ref().map(|f| f.as_str()), Some("eac3"));
    }

    #[test]
    fn png_by_signature() {
        let d = detect_bytes(PNG_1X1, Some("image.png"), cat());
        assert_eq!(d.format.as_ref().unwrap(), "png");
        assert_eq!(d.confidence, Confidence::High);
        assert!(!d.extension_mismatch);
    }

    #[test]
    fn truecolor_tga_is_not_a_cursor() {
        // En-tête TGA 2×1 en couleurs vraies, sans palette ni identifiant : 00 00 02 00 00 …
        let mut tga = vec![0u8, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 1, 0, 24, 0];
        tga.extend_from_slice(&[0, 0, 255, 0, 255, 0]);
        let d = detect_bytes(&tga, Some("image.tga"), cat());
        assert_eq!(d.format.as_ref().map(|f| f.as_str()), Some("tga"), "{d:?}");
        assert!(!d.extension_mismatch);
        assert_eq!(d.confidence, Confidence::Medium, "en-tête cohérent");
        // Des octets quelconques nommés « .tga » restent une simple supposition.
        let d = detect_bytes(&[7u8; 40], Some("faux.tga"), cat());
        assert_eq!(d.confidence, Confidence::Low);
        // Un vrai curseur (une image annoncée) reste un curseur, même mal nommé.
        let mut cur = vec![0u8, 0, 2, 0, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        cur.extend_from_slice(&[0; 40]);
        let d = detect_bytes(&cur, Some("curseur.tga"), cat());
        assert_eq!(d.format.as_ref().map(|f| f.as_str()), Some("cur"), "{d:?}");
    }

    #[test]
    fn mismatched_extension_is_flagged() {
        let d = detect_bytes(PNG_1X1, Some("photo.jpg"), cat());
        assert_eq!(d.format.as_ref().unwrap(), "png");
        assert!(d.extension_mismatch);
    }

    #[test]
    fn container_refined_by_extension() {
        let mut buf = io::Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            z.start_file("page01.png", zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(PNG_1X1).unwrap();
            z.finish().unwrap();
        }
        let bytes = buf.into_inner();
        let d = detect_bytes(&bytes, Some("tome1.cbz"), cat());
        assert_eq!(d.format.as_ref().unwrap(), "cbz");
        assert_eq!(d.method, DetectionMethod::Container);
        let d = detect_bytes(&bytes, Some("archive.zip"), cat());
        assert_eq!(d.format.as_ref().unwrap(), "zip");
    }

    #[test]
    fn docx_from_zip_structure() {
        let mut buf = io::Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            let o = zip::write::SimpleFileOptions::default();
            z.start_file("[Content_Types].xml", o).unwrap();
            z.write_all(br#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#).unwrap();
            z.start_file("word/document.xml", o).unwrap();
            z.write_all(b"<w:document/>").unwrap();
            z.finish().unwrap();
        }
        let d = detect_bytes(&buf.into_inner(), Some("rapport.bin"), cat());
        assert_eq!(d.format.as_ref().unwrap(), "docx");
        assert!(!d.extension_mismatch || d.by_extension.is_none());
    }

    #[test]
    fn text_formats() {
        let c = cat();
        let j = detect_bytes(br#"{"a": [1, 2, {"b": null}]}"#, Some("data.txt"), c);
        assert_eq!(j.format.as_ref().unwrap(), "txt", "l'extension .txt prime, le contenu est signalé");
        assert!(j.note.is_some());
        let j = detect_bytes(br#"{"a": 1}"#, None, c);
        assert_eq!(j.format.as_ref().unwrap(), "json");
        let nd = detect_bytes(b"{\"a\":1}\n{\"a\":2}\n", None, c);
        assert_eq!(nd.format.as_ref().unwrap(), "ndjson");
        let srt = detect_bytes(b"1\n00:00:01,000 --> 00:00:02,000\nBonjour\n", None, c);
        assert_eq!(srt.format.as_ref().unwrap(), "srt");
        let csv = detect_bytes("nom;âge\nAlice;30\nBob;25\n".as_bytes(), None, c);
        assert_eq!(csv.format.as_ref().unwrap(), "csv");
        let tsv = detect_bytes(b"a\tb\n1\t2\n3\t4\n", None, c);
        assert_eq!(tsv.format.as_ref().unwrap(), "tsv");
        let yaml = detect_bytes(b"a: 1\nb: [1, 2]\n", Some("conf.yml"), c);
        assert_eq!(yaml.format.as_ref().unwrap(), "yaml");
        let svg = detect_bytes(br#"<svg xmlns="http://www.w3.org/2000/svg"></svg>"#, Some("x.svg"), c);
        assert_eq!(svg.format.as_ref().unwrap(), "svg");
    }

    #[test]
    fn utf16_text_is_text() {
        let mut bytes = vec![0xFF, 0xFE];
        for u in "a,b\n1,2\n3,4\n".encode_utf16() {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        let d = detect_bytes(&bytes, Some("export.csv"), cat());
        assert_eq!(d.format.as_ref().unwrap(), "csv");
        assert_eq!(d.text_encoding.as_deref(), Some("utf-16le"));
    }

    #[test]
    fn unknown_binary_falls_back_to_extension_with_low_confidence() {
        let bytes: Vec<u8> = (0..4096u32).map(|i| (i * 7919 % 251) as u8).collect();
        let d = detect_bytes(&bytes, Some("scene.blend"), cat());
        assert_eq!(d.format.as_ref().unwrap(), "blend");
        assert_eq!(d.confidence, Confidence::Low);
        let d = detect_bytes(&bytes, None, cat());
        assert!(d.format.is_none());
    }

    #[test]
    fn empty_file() {
        let d = detect_bytes(b"", Some("vide.png"), cat());
        assert!(d.format.is_none());
        assert_eq!(d.note.as_deref(), Some("fichier vide"));
    }
}
