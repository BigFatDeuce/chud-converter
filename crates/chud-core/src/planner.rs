//! Planification des conversions.
//!
//! Les capacités des moteurs forment un graphe (nœuds = formats, arêtes = moteur × conversion).
//! On recherche, pour chaque cible, les chemins de coût minimal avec un ordre lexicographique :
//! 1. nombre d'étapes avec perte ;
//! 2. caractéristiques perdues ;
//! 3. nombre d'étapes ;
//! 4. étapes dont la fidélité dépend du contenu ;
//! 5. coût de préférence des moteurs.
//!
//! L'état de recherche inclut les caractéristiques restantes : comme elles ne peuvent que
//! diminuer le long d'un chemin, la recherche de Dijkstra sur (format, caractéristiques) est exacte.

use crate::engine::{ConversionCap, EngineId, Fidelity};
use crate::feature::{Feature, FeatureSet};
use crate::format::{Category, Compression, FormatCatalog, FormatId};
use serde::Serialize;
use std::cmp::Reverse;
use std::collections::hash_map::Entry;
use std::collections::{BinaryHeap, HashMap};

/// Nombre maximal d'étapes d'un plan (au-delà, la qualité et la lisibilité se dégradent).
pub const DEFAULT_MAX_STEPS: usize = 3;
/// Nombre de plans candidats conservés par cible.
const CANDIDATES_PER_TARGET: usize = 4;

#[derive(Clone, Debug)]
struct Edge {
    engine: EngineId,
    from: FormatId,
    to: FormatId,
    fidelity: Fidelity,
    preserves: FeatureSet,
    cost: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct PlanCost {
    pub lossy_steps: u32,
    pub features_lost: u32,
    pub steps: u32,
    pub depends_steps: u32,
    pub engine_cost: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct PlanStep {
    pub engine: EngineId,
    pub from: FormatId,
    pub to: FormatId,
    pub fidelity: Fidelity,
    pub notes: Vec<String>,
    pub preserves: FeatureSet,
    pub cost: u32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LossCause {
    /// Le format cible ne sait pas représenter cette caractéristique.
    TargetUnsupported,
    /// Le moteur ne la conserve pas, bien que le format cible l'accepte.
    EngineDrops,
}

#[derive(Clone, Debug, Serialize)]
pub struct FeatureLoss {
    pub feature: Feature,
    pub label: String,
    pub step: usize,
    pub cause: LossCause,
    /// Vrai si la sonde a établi que la source contient réellement cette caractéristique.
    pub confirmed: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Warning {
    LossyStep { step: usize, format: FormatId },
    Generational { lossy_steps: u32 },
    Intermediate { formats: Vec<FormatId> },
    ContentDependent { step: usize },
    SourceAlreadyLossy { format: FormatId },
    TargetLimitation { text: String },
    Note { step: usize, text: String },
}

#[derive(Clone, Debug, Serialize)]
pub struct FidelityReport {
    pub overall: Fidelity,
    pub lost_features: Vec<FeatureLoss>,
    pub intermediates: Vec<FormatId>,
    pub warnings: Vec<Warning>,
}

/// Phrase unique qui résume une fidélité, pour une conversion comme pour une opération.
pub(crate) fn headline(overall: Fidelity, loses_information: bool) -> &'static str {
    match overall {
        Fidelity::Lossless if !loses_information => crate::tr!("Sans perte", "Lossless"),
        Fidelity::Lossless => crate::tr!(
            "Contenu sans perte, mais certaines informations ne seront pas conservées",
            "Lossless content, but some information will not be kept"
        ),
        Fidelity::Depends => crate::tr!("Selon le contenu du fichier", "Depends on the file's content"),
        Fidelity::Lossy => crate::tr!("Avec perte", "Lossy"),
    }
}

impl FidelityReport {
    /// Vrai uniquement si aucune étape n'est avec perte ou incertaine ET rien n'est perdu.
    pub fn is_fully_lossless(&self) -> bool {
        self.overall == Fidelity::Lossless && self.lost_features.is_empty()
    }

    pub fn headline(&self) -> &'static str {
        headline(self.overall, !self.lost_features.is_empty())
    }

    pub fn details(&self, catalog: &FormatCatalog) -> Vec<String> {
        let name = |id: &FormatId| catalog.get(id.as_str()).map(|f| f.label().to_owned()).unwrap_or(id.to_string());
        let mut out = Vec::new();
        for w in &self.warnings {
            out.push(match w {
                Warning::LossyStep { step, format } => {
                    crate::trf!("Étape {} : encodage {} avec perte", "Step {}: lossy {} encoding", step + 1, name(format))
                }
                Warning::Generational { lossy_steps } => {
                    crate::trf!(
                        "{lossy_steps} compressions avec perte successives : dégradation cumulée",
                        "{lossy_steps} successive lossy compressions: cumulative degradation"
                    )
                }
                Warning::Intermediate { formats } => {
                    let path = formats.iter().map(&name).collect::<Vec<_>>().join(" → ");
                    crate::trf!("Conversion directe impossible : passage par {path}", "No direct conversion: going through {path}")
                }
                Warning::ContentDependent { step } => {
                    crate::trf!(
                        "Étape {} : la fidélité dépend du contenu du fichier",
                        "Step {}: fidelity depends on the file's content",
                        step + 1
                    )
                }
                Warning::SourceAlreadyLossy { format } => crate::trf!(
                    "La source ({}) est déjà compressée avec perte : la conversion ne restaure pas la qualité d'origine",
                    "The source ({}) is already lossy-compressed: converting does not restore the original quality",
                    name(format)
                ),
                Warning::TargetLimitation { text } => crate::trf!("Limite du format cible : {text}", "Target format limit: {text}"),
                Warning::Note { step, text } => crate::trf!("Étape {} : {text}", "Step {}: {text}", step + 1),
            });
        }
        for l in &self.lost_features {
            let label = l.feature.label();
            let cause = match l.cause {
                LossCause::TargetUnsupported => crate::tr!("non pris en charge par le format cible", "not supported by the target format"),
                LossCause::EngineDrops => crate::tr!("non conservé par le moteur", "not kept by the engine"),
            };
            out.push(if l.confirmed {
                crate::trf!("Perdu : {label} ({cause})", "Lost: {label} ({cause})")
            } else {
                crate::trf!("Perdu si présent : {label} ({cause})", "Lost if present: {label} ({cause})")
            });
        }
        out
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Plan {
    pub source: FormatId,
    pub target: FormatId,
    pub steps: Vec<PlanStep>,
    pub report: FidelityReport,
    pub cost: PlanCost,
}

impl Plan {
    pub fn engines(&self) -> Vec<&EngineId> {
        self.steps.iter().map(|s| &s.engine).collect()
    }

    fn signature(&self) -> Vec<(EngineId, FormatId)> {
        self.steps.iter().map(|s| (s.engine.clone(), s.to.clone())).collect()
    }
}

/// Caractéristiques équivalentes d'une catégorie à l'autre : une vidéo devient une animation
/// (GIF, APNG…) et réciproquement ; des sous-titres extraits deviennent du texte minuté.
const EQUIVALENT: &[(&str, &str)] = &[("video", "animation"), ("subtitles", "timed_text")];

fn supports(set: &FeatureSet, f: &Feature) -> bool {
    set.contains(f)
        || EQUIVALENT
            .iter()
            .any(|(a, b)| (f.as_str() == *a && set.contains(&Feature::new(*b))) || (f.as_str() == *b && set.contains(&Feature::new(*a))))
}

/// Évalue un chemin complet : rapport de fidélité, coût, caractéristiques restantes.
pub fn evaluate(
    catalog: &FormatCatalog,
    source: &FormatId,
    initial: &FeatureSet,
    confirmed: bool,
    steps: &[PlanStep],
) -> (FidelityReport, PlanCost, FeatureSet) {
    let mut remaining = initial.clone();
    let mut lost = Vec::new();
    let mut warnings = Vec::new();
    let mut cost = PlanCost { steps: steps.len() as u32, ..Default::default() };
    let mut overall = Fidelity::Lossless;

    for (i, s) in steps.iter().enumerate() {
        let target_features = catalog.get(s.to.as_str()).map(|f| &f.features);
        for f in remaining.clone() {
            let cause = if !target_features.is_some_and(|tf| supports(tf, &f)) {
                Some(LossCause::TargetUnsupported)
            } else if !supports(&s.preserves, &f) {
                Some(LossCause::EngineDrops)
            } else {
                None
            };
            if let Some(cause) = cause {
                remaining.remove(&f);
                lost.push(FeatureLoss { label: f.label().to_owned(), feature: f, step: i, cause, confirmed });
            }
        }
        match s.fidelity {
            Fidelity::Lossy => {
                cost.lossy_steps += 1;
                warnings.push(Warning::LossyStep { step: i, format: s.to.clone() });
            }
            Fidelity::Depends => {
                cost.depends_steps += 1;
                warnings.push(Warning::ContentDependent { step: i });
            }
            Fidelity::Lossless => {}
        }
        overall = overall.max(s.fidelity);
        cost.engine_cost += s.cost;
        let is_last = i + 1 == steps.len();
        if !is_last && !catalog.get(s.to.as_str()).is_some_and(|f| f.pivot) {
            cost.engine_cost += 5;
        }
        for n in &s.notes {
            warnings.push(Warning::Note { step: i, text: n.clone() });
        }
    }
    cost.features_lost = lost.len() as u32;

    if steps.len() > 1 {
        let formats = steps[..steps.len() - 1].iter().map(|s| s.to.clone()).collect();
        warnings.insert(0, Warning::Intermediate { formats });
    }
    if cost.lossy_steps >= 2 {
        warnings.push(Warning::Generational { lossy_steps: cost.lossy_steps });
    }
    if let (Some(src), Some(last)) = (catalog.get(source.as_str()), steps.last()) {
        let target_lossy = catalog.get(last.to.as_str()).is_some_and(|t| t.compression == Compression::Lossy);
        if src.compression == Compression::Lossy && !target_lossy && last.to != *source {
            warnings.push(Warning::SourceAlreadyLossy { format: source.clone() });
        }
        if let Some(t) = catalog.get(last.to.as_str()) {
            for l in t.limits() {
                warnings.push(Warning::TargetLimitation { text: l.clone() });
            }
        }
    }

    let intermediates = steps.iter().take(steps.len().saturating_sub(1)).map(|s| s.to.clone()).collect();
    (FidelityReport { overall, lost_features: lost, intermediates, warnings }, cost, remaining)
}

/// Contenu sans lequel un format cible n'a pas de sens. Le planificateur ne propose pas
/// « l'audio » d'une image ni « les sous-titres » d'un fichier qui n'en contient pas, même si une
/// chaîne de moteurs (image → GIF → vidéo → audio) le permettrait sur le papier.
fn has_essential_content(catalog: &FormatCatalog, target: &FormatId, present: &FeatureSet) -> bool {
    let Some(def) = catalog.get(target.as_str()) else { return true };
    // Un APNG tiré d'une image fixe, ou passé par un PNG intermédiaire, n'aurait qu'une image.
    if !def.requires.iter().all(|f| supports(present, f)) {
        return false;
    }
    let needed: &[&str] = match def.category {
        Category::Audio => &["audio", "notation"],
        Category::Subtitle => &["subtitles", "timed_text", "image_subtitles"],
        _ => return true,
    };
    needed.iter().any(|n| present.contains(&Feature::new(*n)))
}

/// Graphe de conversions construit à partir des capacités des moteurs actifs.
pub struct Planner<'a> {
    catalog: &'a FormatCatalog,
    edges: Vec<Edge>,
    out: HashMap<FormatId, Vec<usize>>,
    max_steps: usize,
}

impl<'a> Planner<'a> {
    pub fn new(catalog: &'a FormatCatalog) -> Self {
        Self { catalog, edges: Vec::new(), out: HashMap::new(), max_steps: DEFAULT_MAX_STEPS }
    }

    pub fn with_max_steps(mut self, n: usize) -> Self {
        self.max_steps = n.max(1);
        self
    }

    /// Ajoute les arêtes d'un moteur. Les formats absents du catalogue sont ignorés.
    pub fn add_engine(&mut self, engine: &EngineId, caps: &[ConversionCap]) {
        for c in caps {
            if self.catalog.get(c.from.as_str()).is_none() || self.catalog.get(c.to.as_str()).is_none() {
                continue;
            }
            self.out.entry(c.from.clone()).or_default().push(self.edges.len());
            self.edges.push(Edge {
                engine: engine.clone(),
                from: c.from.clone(),
                to: c.to.clone(),
                fidelity: c.fidelity,
                preserves: c.preserves.clone(),
                cost: c.cost,
            });
        }
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    fn step(&self, ei: usize) -> PlanStep {
        let e = &self.edges[ei];
        PlanStep {
            engine: e.engine.clone(),
            from: e.from.clone(),
            to: e.to.clone(),
            fidelity: e.fidelity,
            notes: Vec::new(),
            preserves: e.preserves.clone(),
            cost: e.cost,
        }
    }

    /// Calcule les meilleurs plans vers toutes les cibles atteignables depuis `source`.
    /// `source_features` : caractéristiques réellement présentes si une sonde les a établies ;
    /// sinon on raisonne sur les caractéristiques potentielles du format source.
    pub fn plan_all(&self, source: &FormatId, source_features: Option<&FeatureSet>) -> HashMap<FormatId, Vec<Plan>> {
        let Some(src_def) = self.catalog.get(source.as_str()) else {
            return HashMap::new();
        };
        let (initial, confirmed) = match source_features {
            Some(f) => (f.clone(), true),
            None => (src_def.features.clone(), false),
        };

        struct Node {
            format: FormatId,
            path: Vec<usize>,
            remaining: FeatureSet,
        }
        let mut nodes = vec![Node { format: source.clone(), path: Vec::new(), remaining: initial.clone() }];
        let mut heap = BinaryHeap::new();
        heap.push(Reverse((PlanCost::default(), 0usize)));
        // État exploré → plus petite profondeur à laquelle il l'a été. Un état déjà atteint par un
        // chemin moins coûteux mais plus long doit encore être exploré depuis un chemin plus court :
        // il lui reste plus d'étapes pour atteindre d'autres cibles.
        let mut settled: HashMap<(FormatId, FeatureSet), usize> = HashMap::new();
        let mut candidates: HashMap<FormatId, Vec<(PlanCost, Vec<usize>)>> = HashMap::new();

        while let Some(Reverse((_, idx))) = heap.pop() {
            let (format, path, remaining) = {
                let n = &nodes[idx];
                (n.format.clone(), n.path.clone(), n.remaining.clone())
            };
            if path.len() >= self.max_steps {
                continue;
            }
            match settled.entry((format.clone(), remaining)) {
                Entry::Occupied(e) if *e.get() <= path.len() => continue,
                Entry::Occupied(mut e) => {
                    e.insert(path.len());
                }
                Entry::Vacant(e) => {
                    e.insert(path.len());
                }
            }
            for &ei in self.out.get(&format).map(Vec::as_slice).unwrap_or(&[]) {
                let e = &self.edges[ei];
                // Réencodage dans le même format : uniquement en conversion directe.
                if e.to == *source {
                    if path.is_empty() && e.from == e.to {
                        let steps = vec![self.step(ei)];
                        let (_, cost, rem) = evaluate(self.catalog, source, &initial, confirmed, &steps);
                        if !has_essential_content(self.catalog, &e.to, &rem) {
                            continue;
                        }
                        candidates.entry(e.to.clone()).or_default().push((cost, vec![ei]));
                    }
                    continue;
                }
                if e.from == e.to || path.iter().any(|&p| self.edges[p].to == e.to) {
                    continue;
                }
                let mut new_path = path.clone();
                new_path.push(ei);
                let steps: Vec<PlanStep> = new_path.iter().map(|&i| self.step(i)).collect();
                let (_, cost, rem) = evaluate(self.catalog, source, &initial, confirmed, &steps);
                if has_essential_content(self.catalog, &e.to, &rem) {
                    candidates.entry(e.to.clone()).or_default().push((cost.clone(), new_path.clone()));
                }
                // Les variantes (ALAC, PDF/A…) et les formats « source seulement » (CBZ…) ne
                // servent jamais d'intermédiaire.
                if self.catalog.get(e.to.as_str()).is_some_and(|f| f.can_be_intermediate()) {
                    nodes.push(Node { format: e.to.clone(), path: new_path, remaining: rem });
                    heap.push(Reverse((cost, nodes.len() - 1)));
                }
            }
        }

        let mut result = HashMap::new();
        for (target, mut list) in candidates {
            list.sort_by(|a, b| a.0.cmp(&b.0));
            let mut plans: Vec<Plan> = Vec::new();
            for (_, path) in list {
                let steps: Vec<PlanStep> = path.iter().map(|&i| self.step(i)).collect();
                let (report, cost, _) = evaluate(self.catalog, source, &initial, confirmed, &steps);
                let plan = Plan { source: source.clone(), target: target.clone(), steps, report, cost };
                if plans.iter().any(|p| p.signature() == plan.signature()) {
                    continue;
                }
                plans.push(plan);
                if plans.len() >= CANDIDATES_PER_TARGET {
                    break;
                }
            }
            result.insert(target, plans);
        }
        result
    }

    pub fn plan(&self, source: &FormatId, source_features: Option<&FeatureSet>, target: &FormatId) -> Vec<Plan> {
        self.plan_all(source, source_features).remove(target).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feature::features;

    fn cat() -> &'static FormatCatalog {
        FormatCatalog::builtin()
    }

    fn cap(from: &str, to: &str, f: Fidelity, keep: &[&str], cost: u32) -> ConversionCap {
        ConversionCap::new(from, to, f, &features(keep), cost)
    }

    #[test]
    fn a_cheaper_but_longer_path_does_not_hide_targets_of_a_shorter_one() {
        // PNG → TIFF → WebP (sans perte, 2 étapes) arrive avant PNG → WebP (avec perte, 1 étape)
        // au même état. Avec 3 étapes au plus, seul le chemin court peut aller jusqu'à l'ICO.
        let mut p = Planner::new(cat());
        p.add_engine(
            &EngineId::new("a"),
            &[
                cap("png", "tiff", Fidelity::Lossless, &["alpha"], 1),
                cap("tiff", "webp", Fidelity::Lossless, &["alpha"], 1),
                cap("png", "webp", Fidelity::Lossy, &["alpha"], 1),
                cap("webp", "gif", Fidelity::Lossless, &["alpha"], 1),
                cap("gif", "ico", Fidelity::Lossless, &["alpha"], 1),
            ],
        );
        let all = p.plan_all(&"png".into(), None);
        let ico = all.get(&FormatId::new("ico")).expect("l'ICO est atteignable en 3 étapes");
        assert_eq!(ico[0].steps.len(), 3);
    }

    /// Cibles atteignables par énumération de tous les chemins, avec les mêmes règles que
    /// `plan_all` (pas de retour sur un format déjà produit, intermédiaires autorisés seulement).
    fn exhaustive_targets(p: &Planner, source: &FormatId) -> std::collections::BTreeSet<String> {
        fn walk(
            p: &Planner,
            source: &FormatId,
            initial: &FeatureSet,
            path: &mut Vec<usize>,
            found: &mut std::collections::BTreeSet<String>,
        ) {
            let format = path.last().map_or(source.clone(), |&i| p.edges[i].to.clone());
            if path.len() >= p.max_steps {
                return;
            }
            for &ei in p.out.get(&format).map(Vec::as_slice).unwrap_or(&[]) {
                let e = &p.edges[ei];
                if e.to == *source || e.from == e.to || path.iter().any(|&q| p.edges[q].to == e.to) {
                    continue;
                }
                path.push(ei);
                let steps: Vec<PlanStep> = path.iter().map(|&i| p.step(i)).collect();
                let (_, _, rem) = evaluate(p.catalog, source, initial, false, &steps);
                if has_essential_content(p.catalog, &e.to, &rem) {
                    found.insert(e.to.as_str().to_owned());
                }
                if p.catalog.get(e.to.as_str()).is_some_and(|f| f.can_be_intermediate()) {
                    walk(p, source, initial, path, found);
                }
                path.pop();
            }
        }
        let initial = p.catalog.get(source.as_str()).unwrap().features.clone();
        let mut found = std::collections::BTreeSet::new();
        walk(p, source, &initial, &mut Vec::new(), &mut found);
        found
    }

    #[test]
    fn finds_every_target_an_exhaustive_search_finds() {
        // Petits graphes pseudo-aléatoires (reproductibles) : fidélités, coûts et caractéristiques
        // conservées varient, pour que des chemins différents arrivent au même état.
        let formats = ["png", "tiff", "webp", "gif", "ico", "bmp", "jpg", "mkv"];
        let keeps: [&[&str]; 4] = [&["alpha"], &["alpha", "animation"], &[], &["animation", "audio", "video"]];
        let fids = [Fidelity::Lossless, Fidelity::Lossy, Fidelity::Depends];
        let mut seed: u64 = 0x5eed;
        let mut next = |n: usize| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 33) as usize) % n
        };
        for round in 0..3000 {
            let mut p = Planner::new(cat());
            let caps: Vec<ConversionCap> = (0..6 + next(10))
                .map(|_| cap(formats[next(formats.len())], formats[next(formats.len())], fids[next(3)], keeps[next(4)], 1 + next(5) as u32))
                .collect();
            p.add_engine(&EngineId::new("e"), &caps);
            for src in ["png", "gif", "mkv"] {
                let source = FormatId::new(src);
                let planned: std::collections::BTreeSet<String> =
                    p.plan_all(&source, None).into_keys().filter(|t| *t != source).map(|t| t.as_str().to_owned()).collect();
                assert_eq!(planned, exhaustive_targets(&p, &source), "graphe n°{round}, source {src} : {caps:?}");
            }
        }
    }

    #[test]
    fn prefers_lossless_direct_path() {
        let mut p = Planner::new(cat());
        p.add_engine(&EngineId::new("a"), &[cap("png", "bmp", Fidelity::Lossless, &["alpha"], 10)]);
        p.add_engine(&EngineId::new("b"), &[cap("png", "bmp", Fidelity::Lossy, &["alpha"], 1)]);
        let plans = p.plan(&"png".into(), None, &"bmp".into());
        assert_eq!(plans[0].steps[0].engine.as_str(), "a");
        assert_eq!(plans.len(), 2, "l'alternative est conservée");
    }

    #[test]
    fn never_offers_audio_or_subtitles_that_cannot_exist() {
        // Chaîne réelle des moteurs : image → GIF → MKV → MP3 / SRT.
        let mut p = Planner::new(cat());
        p.add_engine(&EngineId::new("img"), &[cap("png", "gif", Fidelity::Lossy, &["alpha"], 1)]);
        p.add_engine(
            &EngineId::new("av"),
            &[
                cap("gif", "mkv", Fidelity::Lossy, &["animation"], 1),
                cap("mkv", "mp3", Fidelity::Lossy, &["audio", "tags"], 1),
                cap("mkv", "srt", Fidelity::Depends, &["subtitles", "timed_text"], 1),
            ],
        );
        let all = p.plan_all(&"png".into(), None);
        assert!(all.contains_key(&FormatId::new("mkv")), "une image peut devenir une vidéo d'une image");
        assert!(!all.contains_key(&FormatId::new("mp3")), "une image n'a pas de son");
        assert!(!all.contains_key(&FormatId::new("srt")), "une image n'a pas de sous-titres");

        // Une vraie vidéo garde ces cibles… sauf si la sonde montre qu'elle n'a pas de piste audio.
        let all = p.plan_all(&"mkv".into(), None);
        assert!(all.contains_key(&FormatId::new("mp3")) && all.contains_key(&FormatId::new("srt")));
        let silent = features(&["video"]);
        let all = p.plan_all(&"mkv".into(), Some(&silent));
        assert!(!all.contains_key(&FormatId::new("mp3")), "vidéo muette : pas d'extraction audio");
    }

    #[test]
    fn never_offers_an_animation_that_cannot_exist() {
        // Chaîne réelle : PCX → PNG → WebP → APNG donnait un PNG ordinaire nommé « .apng ».
        let mut p = Planner::new(cat());
        p.add_engine(&EngineId::new("im"), &[cap("pcx", "png", Fidelity::Depends, &["alpha"], 1)]);
        p.add_engine(
            &EngineId::new("img"),
            &[cap("png", "webp", Fidelity::Lossless, &["alpha", "animation"], 1), cap("gif", "png", Fidelity::Lossless, &["alpha"], 1)],
        );
        p.add_engine(
            &EngineId::new("av"),
            &[
                cap("webp", "apng", Fidelity::Lossless, &["alpha", "animation"], 1),
                cap("gif", "apng", Fidelity::Lossless, &["alpha", "animation"], 1),
                cap("mp4", "apng", Fidelity::Lossy, &["animation"], 1),
            ],
        );
        assert!(!p.plan_all(&"pcx".into(), None).contains_key(&FormatId::new("apng")), "un PCX n'est jamais animé");
        assert!(p.plan_all(&"webp".into(), None).contains_key(&FormatId::new("apng")), "un WebP peut l'être");
        let still = features(&["alpha"]);
        assert!(!p.plan_all(&"webp".into(), Some(&still)).contains_key(&FormatId::new("apng")), "WebP fixe d'après la sonde");
        let gif = p.plan_all(&"gif".into(), None);
        assert!(gif[&FormatId::new("apng")].iter().all(|pl| pl.steps.len() == 1), "jamais par un PNG intermédiaire");
        assert!(p.plan_all(&"mp4".into(), None).contains_key(&FormatId::new("apng")), "une vidéo devient une animation");
    }

    #[test]
    fn uses_intermediate_when_needed_and_says_so() {
        let mut p = Planner::new(cat());
        p.add_engine(&EngineId::new("x"), &[cap("cr2", "tiff", Fidelity::Lossy, &[], 1)]);
        p.add_engine(&EngineId::new("y"), &[cap("tiff", "webp", Fidelity::Lossy, &[], 1)]);
        let plans = p.plan(&"cr2".into(), None, &"webp".into());
        let plan = &plans[0];
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.report.intermediates, vec![FormatId::new("tiff")]);
        assert_eq!(plan.report.overall, Fidelity::Lossy);
        assert!(plan.report.warnings.iter().any(|w| matches!(w, Warning::Generational { lossy_steps: 2 })));
    }

    #[test]
    fn never_claims_lossless_when_a_step_is_lossy() {
        let mut p = Planner::new(cat());
        p.add_engine(&EngineId::new("x"), &[cap("png", "tiff", Fidelity::Lossless, &["alpha"], 1)]);
        p.add_engine(&EngineId::new("y"), &[cap("tiff", "jpeg", Fidelity::Lossy, &[], 1)]);
        let plan = &p.plan(&"png".into(), None, &"jpeg".into())[0];
        assert_eq!(plan.report.overall, Fidelity::Lossy);
        assert!(!plan.report.is_fully_lossless());
    }

    #[test]
    fn reports_feature_losses_with_cause() {
        let mut p = Planner::new(cat());
        p.add_engine(&EngineId::new("x"), &[cap("png", "jpeg", Fidelity::Lossy, &[], 1)]);
        let src = features(&["alpha", "exif"]);
        let plan = &p.plan(&"png".into(), Some(&src), &"jpeg".into())[0];
        let alpha = plan.report.lost_features.iter().find(|l| l.feature.as_str() == "alpha").unwrap();
        assert!(matches!(alpha.cause, LossCause::TargetUnsupported));
        assert!(alpha.confirmed);
        let exif = plan.report.lost_features.iter().find(|l| l.feature.as_str() == "exif").unwrap();
        assert!(matches!(exif.cause, LossCause::EngineDrops), "JPEG accepte l'EXIF mais le moteur le perd");
    }

    #[test]
    fn fewer_feature_losses_beat_cheaper_engine() {
        let mut p = Planner::new(cat());
        p.add_engine(&EngineId::new("cheap"), &[cap("png", "webp", Fidelity::Lossless, &[], 1)]);
        p.add_engine(&EngineId::new("careful"), &[cap("png", "webp", Fidelity::Lossless, &["alpha", "icc", "exif", "xmp"], 50)]);
        let plan = &p.plan(&"png".into(), Some(&features(&["alpha", "icc"])), &"webp".into())[0];
        assert_eq!(plan.steps[0].engine.as_str(), "careful");
        assert!(plan.report.is_fully_lossless());
    }

    #[test]
    fn respects_max_steps_and_no_cycles() {
        let mut p = Planner::new(cat()).with_max_steps(2);
        p.add_engine(
            &EngineId::new("x"),
            &[
                cap("bmp", "png", Fidelity::Lossless, &[], 1),
                cap("png", "tga", Fidelity::Lossless, &[], 1),
                cap("tga", "qoi", Fidelity::Lossless, &[], 1),
                cap("png", "bmp", Fidelity::Lossless, &[], 1),
            ],
        );
        let all = p.plan_all(&"bmp".into(), None);
        assert!(all.contains_key(&FormatId::new("tga")));
        assert!(!all.contains_key(&FormatId::new("qoi")), "3 étapes > maximum de 2");
        assert!(!all.contains_key(&FormatId::new("bmp")), "pas de retour au format source");
    }

    #[test]
    fn same_format_reencode_is_direct_only() {
        let mut p = Planner::new(cat());
        p.add_engine(&EngineId::new("x"), &[cap("jpeg", "jpeg", Fidelity::Lossy, &[], 1)]);
        let plans = p.plan(&"jpeg".into(), None, &"jpeg".into());
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].steps.len(), 1);
    }

    #[test]
    fn warns_when_source_is_already_lossy() {
        let mut p = Planner::new(cat());
        p.add_engine(&EngineId::new("x"), &[cap("mp3", "flac", Fidelity::Lossless, &["audio", "tags"], 1)]);
        let plan = &p.plan(&"mp3".into(), None, &"flac".into())[0];
        assert!(plan.report.warnings.iter().any(|w| matches!(w, Warning::SourceAlreadyLossy { .. })));
    }

    #[test]
    fn comic_books_are_never_intermediates() {
        let mut p = Planner::new(cat());
        p.add_engine(
            &EngineId::new("x"),
            &[cap("zip", "cbz", Fidelity::Lossless, &["images"], 1), cap("cbz", "epub", Fidelity::Lossy, &["images"], 1)],
        );
        let all = p.plan_all(&"zip".into(), None);
        assert!(all.contains_key(&FormatId::new("cbz")));
        assert!(!all.contains_key(&FormatId::new("epub")));
        assert!(p.plan_all(&"cbz".into(), None).contains_key(&FormatId::new("epub")));
    }

    #[test]
    fn variants_are_not_intermediates() {
        let mut p = Planner::new(cat());
        p.add_engine(
            &EngineId::new("x"),
            &[cap("wav", "alac", Fidelity::Lossless, &["audio"], 1), cap("alac", "flac", Fidelity::Lossless, &["audio"], 1)],
        );
        let all = p.plan_all(&"wav".into(), None);
        assert!(all.contains_key(&FormatId::new("alac")));
        assert!(!all.contains_key(&FormatId::new("flac")));
    }
}
