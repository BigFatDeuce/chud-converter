//! Lecture de texte avec détection d'encodage (BOM, UTF-8 strict, sinon détection statistique).

use encoding_rs::Encoding;

pub struct DecodedText {
    pub text: String,
    /// Nom de l'encodage détecté (« UTF-8 », « windows-1252 »…).
    pub encoding: &'static str,
    pub had_bom: bool,
    /// Vrai si des octets invalides ont été remplacés.
    pub lossy: bool,
}

pub fn decode(bytes: &[u8]) -> DecodedText {
    if let Some((enc, bom_len)) = Encoding::for_bom(bytes) {
        let (text, had_errors) = enc.decode_without_bom_handling(&bytes[bom_len..]);
        return DecodedText { text: text.into_owned(), encoding: enc.name(), had_bom: true, lossy: had_errors };
    }
    if let Ok(s) = std::str::from_utf8(bytes) {
        return DecodedText { text: s.to_owned(), encoding: "UTF-8", had_bom: false, lossy: false };
    }
    let mut det = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Deny);
    det.feed(bytes, true);
    let enc = det.guess(None, chardetng::Utf8Detection::Allow);
    let (text, _, had_errors) = enc.decode(bytes);
    DecodedText { text: text.into_owned(), encoding: enc.name(), had_bom: false, lossy: had_errors }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_windows_1252() {
        // « Café;Crème » en Windows-1252 (export Excel français typique).
        let bytes = b"Nom;Boisson\nZo\xe9;Caf\xe9 cr\xe8me\nL\xe9a;Th\xe9 \xe0 la menthe\n";
        let d = decode(bytes);
        assert!(d.text.contains("Café crème"), "{}", d.text);
        assert_ne!(d.encoding, "UTF-8");
    }

    #[test]
    fn strips_utf8_bom() {
        let d = decode(b"\xEF\xBB\xBFa,b");
        assert_eq!(d.text, "a,b");
        assert!(d.had_bom);
    }
}
