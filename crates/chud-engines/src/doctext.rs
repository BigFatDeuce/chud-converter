//! Relecture du texte des documents bureautiques, pour vérifier qu'une conversion n'a pas perdu
//! le contenu : les mots du document d'origine doivent se retrouver dans le résultat.
//!
//! Lecture volontairement simple (balises retirées, entités décodées) et indépendante des moteurs
//! de conversion. Elle ne juge que le texte : la mise en page, les images et les formules restent
//! hors de sa portée, et le rapport le dit.

use chud_core::trf;
use std::collections::HashSet;
use std::io::Read;
use std::path::Path;

/// Au plus 64 Mo de texte lus par document (protection contre les archives piégées).
const MAX_READ: u64 = 64 * 1024 * 1024;
/// En dessous de ce nombre de mots distincts, la comparaison ne dit rien d'utile.
const MIN_WORDS: usize = 5;
/// Part des mots d'origine qui doit se retrouver dans le résultat.
const MIN_RECALL: f64 = 0.9;

/// Résultat de la comparaison : `None` si l'un des deux formats n'est pas lisible ici ou si
/// l'original a trop peu de texte ; sinon le constat, réussi ou non.
pub(crate) fn compare(input: &Path, from: &str, output: &Path, to: &str) -> Option<Result<String, String>> {
    let source = words(&text_of(input, from, Part::Main)?);
    if source.len() < MIN_WORDS {
        return None;
    }
    // Un résultat illisible a déjà été refusé par le contrôle de structure.
    let result = words(&text_of(output, to, Part::All)?);
    let found = source.iter().filter(|w| result.contains(*w)).count();
    let total = source.len();
    let pct = found * 100 / total;
    if (found as f64) < total as f64 * MIN_RECALL {
        let missing: Vec<&str> = source.iter().filter(|w| !result.contains(*w)).take(5).map(String::as_str).collect();
        return Some(Err(trf!(
            "seulement {pct} % des mots du document d'origine retrouvés ({found} sur {total} ; absents : {})",
            "only {pct}% of the original document's words found ({found} of {total}; missing: {})",
            missing.join(", ")
        )));
    }
    Some(Ok(trf!(
        "texte relu : {pct} % des {total} mots du document d'origine retrouvés",
        "text re-read: {pct}% of the original document's {total} words found"
    )))
}

#[derive(Clone, Copy, PartialEq)]
enum Part {
    /// Le corps du document seulement (côté original).
    Main,
    /// Tout ce qui peut porter du texte (côté résultat : en-têtes, notes, commentaires, et les
    /// métadonnées, où un titre peut légitimement passer).
    All,
}

fn text_of(path: &Path, format: &str, part: Part) -> Option<String> {
    let main = part == Part::Main;
    match format {
        "docx" => zip_text(path, |n| {
            if main { n == "word/document.xml" } else { (n.starts_with("word/") && n.ends_with(".xml")) || n == "docProps/core.xml" }
        }),
        "xlsx" => zip_text(path, |n| {
            if main {
                n == "xl/sharedStrings.xml" || (n.starts_with("xl/worksheets/") && n.ends_with(".xml"))
            } else {
                (n.starts_with("xl/") && n.ends_with(".xml")) || n == "docProps/core.xml"
            }
        }),
        "pptx" => zip_text(path, |n| {
            if main {
                n.starts_with("ppt/slides/slide") && n.ends_with(".xml")
            } else {
                (n.starts_with("ppt/") && n.ends_with(".xml")) || n == "docProps/core.xml"
            }
        }),
        "odt" | "ods" | "odp" => zip_text(path, |n| n == "content.xml" || (!main && (n == "styles.xml" || n == "meta.xml"))),
        "epub" if main => zip_text(path, |n| {
            let n = n.to_ascii_lowercase();
            n.ends_with(".xhtml") || n.ends_with(".html") || n.ends_with(".htm")
        }),
        "html" if main => read_limited(std::fs::File::open(path).ok()?).map(|b| markup_text(&b)),
        "txt" | "md" | "csv" if main => read_limited(std::fs::File::open(path).ok()?).map(|b| String::from_utf8_lossy(&b).into_owned()),
        _ => None,
    }
}

fn zip_text(path: &Path, wanted: impl Fn(&str) -> bool) -> Option<String> {
    let mut z = zip::ZipArchive::new(std::fs::File::open(path).ok()?).ok()?;
    let mut text = String::new();
    let mut budget = MAX_READ;
    for i in 0..z.len() {
        let m = z.by_index(i).ok()?;
        // Nom illisible (encodage invalide) : membre ignoré.
        if !m.name().is_ok_and(|n| wanted(&n)) {
            continue;
        }
        let mut buf = Vec::new();
        m.take(budget).read_to_end(&mut buf).ok()?;
        budget = budget.saturating_sub(buf.len() as u64);
        text.push_str(&markup_text(&buf));
        text.push(' ');
        if budget == 0 {
            break;
        }
    }
    Some(text)
}

fn read_limited(f: std::fs::File) -> Option<Vec<u8>> {
    let mut buf = Vec::new();
    f.take(MAX_READ).read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// Éléments qui séparent des mots (paragraphes, cellules, sauts, tabulations…). Les autres
/// balises (fragments de mise en forme) sont retirées sans espace : un mot découpé en plusieurs
/// morceaux reste entier.
fn separates(local: &str) -> bool {
    matches!(
        local,
        "p" | "h"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "tab"
            | "br"
            | "s"
            | "line-break"
            | "si"
            | "c"
            | "v"
            | "table-cell"
            | "cr"
            | "li"
            | "td"
            | "th"
            | "tr"
            | "div"
            | "title"
            | "row"
            | "sp"
            | "txbody"
    )
}

/// Texte d'un document XML ou HTML : balises retirées, entités décodées ; contenu des éléments
/// `style`, `script` et `head` (titre technique d'une page, jamais affiché) ignoré.
fn markup_text(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    let mut out = String::with_capacity(s.len() / 2);
    let mut rest: &str = &s;
    while let Some(lt) = rest.find('<') {
        push_decoded(&mut out, &rest[..lt]);
        let Some(gt) = rest[lt..].find('>') else { break };
        let tag = &rest[lt + 1..lt + gt];
        rest = &rest[lt + gt + 1..];
        let name = tag.trim_start_matches('/').split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or("");
        let local = name.rsplit(':').next().unwrap_or(name).to_ascii_lowercase();
        if !tag.starts_with('/') && !tag.ends_with('/') && (local == "style" || local == "script" || local == "head") {
            let close = format!("</{name}");
            match rest.to_ascii_lowercase().find(&close.to_ascii_lowercase()) {
                Some(i) => rest = &rest[i..],
                None => break,
            }
            continue;
        }
        if separates(&local) {
            out.push(' ');
        }
    }
    push_decoded(&mut out, rest);
    out
}

fn push_decoded(out: &mut String, text: &str) {
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        let end = after.find(';').filter(|&i| i <= 10);
        let decoded = end.and_then(|i| match &after[..i] {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            e if e.starts_with("#x") || e.starts_with("#X") => u32::from_str_radix(&e[2..], 16).ok().and_then(char::from_u32),
            e if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        });
        match (decoded, end) {
            (Some(c), Some(i)) => {
                out.push(c);
                rest = &after[i + 1..];
            }
            _ => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
}

/// Mots distincts d'au moins 4 lettres, en minuscules. Les nombres sont écartés : leur écriture
/// change légitimement (« 3,5 » / « 3.5 », dates).
fn words(text: &str) -> HashSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 4 && w.chars().all(char::is_alphabetic))
        .map(str::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn pack(path: &Path, parts: &[(&str, &str)]) {
        let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for (name, body) in parts {
            z.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(body.as_bytes()).unwrap();
        }
        z.finish().unwrap();
    }

    #[test]
    fn markup_is_read_like_text() {
        // Mot découpé en morceaux de mise en forme, paragraphes, entités, CSS ignorée.
        let t = markup_text(
            br#"<html><head><title>input</title><style>p { margin: 0 }</style></head><w:p><w:r><w:t>Bon</w:t></w:r><w:r><w:t>jour</w:t></w:r></w:p><w:p><w:t>caf&#233; &amp; th&eacute;</w:t></w:p></html>"#,
        );
        assert!(t.contains("Bonjour"), "{t}");
        assert!(t.contains("café &"), "{t}");
        assert!(!t.contains("margin") && !t.contains("input"), "{t}");
        let w = words("Le Café, 3.5 kg — les cafés : CAFÉ 2026 x1y2");
        assert_eq!(w, ["café", "cafés"].into_iter().map(String::from).collect());
    }

    #[test]
    fn missing_text_is_caught_and_kept_text_is_confirmed() {
        let dir = std::env::temp_dir().join(format!("chud-doctext-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let odt = dir.join("lettre.odt");
        pack(
            &odt,
            &[(
                "content.xml",
                "<office:text><text:h>Rapport annuel</text:h><text:p>Les résultats de cette année dépassent les prévisions établies.</text:p></office:text>",
            )],
        );
        let good = dir.join("bon.docx");
        pack(
            &good,
            &[
                (
                    "word/document.xml",
                    "<w:p><w:t>Rapport annuel</w:t></w:p><w:p><w:t>Les résultats de cette année dépassent les prévisions</w:t></w:p>",
                ),
                ("word/footer1.xml", "<w:p><w:t>établies</w:t></w:p>"),
            ],
        );
        let bad = dir.join("vide.docx");
        pack(&bad, &[("word/document.xml", "<w:p><w:t>Rapport</w:t></w:p>")]);
        let ok = compare(&odt, "odt", &good, "docx").unwrap();
        assert!(ok.as_ref().is_ok_and(|s| s.contains("100")), "{ok:?}");
        let ko = compare(&odt, "odt", &bad, "docx").unwrap();
        assert!(ko.as_ref().is_err_and(|e| e.contains("annuel") || e.contains("résultats")), "{ko:?}");
        // Formats non lus ici, ou original presque vide : pas de verdict.
        assert!(compare(&odt, "odt", &good, "pdf").is_none());
        assert!(compare(&good, "doc", &good, "docx").is_none());
        let tiny = dir.join("court.txt");
        std::fs::write(&tiny, "Bonjour").unwrap();
        assert!(compare(&tiny, "txt", &good, "docx").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
