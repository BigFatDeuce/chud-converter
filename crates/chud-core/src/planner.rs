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
use std::collections::{BinaryHeap, HashMap, HashSet};

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

impl FidelityReport {
    /// Vrai uniquement si aucune étape n'est avec perte ou incertaine ET rien n'est perdu.
    pub fn is_fully_lossless(&self) -> bool {
        self.overall == Fidelity::Lossless && self.lost_features.is_empty()
    }

    pub fn headline_fr(&self) -> &'static str {
        match self.overall {
            Fidelity::Lossless if self.lost_features.is_empty() => "Sans perte",
            Fidelity::Lossless => "Contenu sans perte, mais certaines informations ne seront pas conservées",
            Fidelity::Depends => "Selon le contenu du fichier",
            Fidelity::Lossy => "Avec perte",
        }
    }

    pub fn details_fr(&self, catalog: &FormatCatalog) -> Vec<String> {
        let name = |id: &FormatId| catalog.get(id.as_str()).map(|f| f.name.clone()).unwrap_or(id.to_string());
        let mut out = Vec::new();
        for w in &self.warnings {
            out.push(match w {
                Warning::LossyStep { step, format } => {
                    format!("Étape {} : encodage {} avec perte", step + 1, name(format))
                }
                Warning::Generational { lossy_steps } => {
                    format!("{lossy_steps} compressions avec perte successives : dégradation cumulée")
                }
                Warning::Intermediate { formats } => {
                    format!("Conversion directe impossible : passage par {}", formats.iter().map(&name).collect::<Vec<_>>().join(" → "))
                }
                Warning::ContentDependent { step } => {
                    format!("Étape {} : la fidélité dépend du contenu du fichier", step + 1)
                }
                Warning::SourceAlreadyLossy { format } => format!(
                    "La source ({}) est déjà compressée avec perte : la conversion ne restaure pas la qualité d'origine",
                    name(format)
                ),
                Warning::TargetLimitation { text } => format!("Limite du format cible : {text}"),
                Warning::Note { step, text } => format!("Étape {} : {text}", step + 1),
            });
        }
        for l in &self.lost_features {
            let prefix = if l.confirmed { "Perdu" } else { "Perdu si présent" };
            let cause = match l.cause {
                LossCause::TargetUnsupported => "non pris en charge par le format cible",
                LossCause::EngineDrops => "non conservé par le moteur",
            };
            out.push(format!("{prefix} : {} ({cause})", l.label));
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
                lost.push(FeatureLoss { label: f.label_fr().to_owned(), feature: f, step: i, cause, confirmed });
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
            for l in &t.limitations {
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
    let needed: &[&str] = match catalog.get(target.as_str()).map(|f| f.category) {
        Some(Category::Audio) => &["audio", "notation"],
        Some(Category::Subtitle) => &["subtitles", "timed_text", "image_subtitles"],
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
        let mut settled: HashSet<(FormatId, FeatureSet)> = HashSet::new();
        let mut candidates: HashMap<FormatId, Vec<(PlanCost, Vec<usize>)>> = HashMap::new();

        while let Some(Reverse((_, idx))) = heap.pop() {
            let (format, path, remaining) = {
                let n = &nodes[idx];
                (n.format.clone(), n.path.clone(), n.remaining.clone())
            };
            if !settled.insert((format.clone(), remaining)) || path.len() >= self.max_steps {
                continue;
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
                // Les variantes (ALAC, PDF/A…) ne servent jamais d'intermédiaire.
                if !self.catalog.get(e.to.as_str()).is_some_and(|f| f.is_variant()) {
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
