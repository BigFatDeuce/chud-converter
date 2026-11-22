//! Registre des moteurs actifs : construit le graphe, sonde les fichiers et affine les plans
//! avec l'évaluation précise de chaque moteur (`Engine::assess`).

use crate::engine::{Engine, EngineId, Fidelity, SourceInfo, StepQuery};
use crate::format::{Category, FormatCatalog, FormatId};
use crate::options::OptionValues;
use crate::planner::{Plan, Planner, evaluate};
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error, Clone, Serialize)]
pub enum PlanError {
    #[error("{t}{0}", t = crate::tr!("format source inconnu : ", "unknown source format: "))]
    UnknownSource(String),
    #[error("{t}{0}", t = crate::tr!("format cible inconnu : ", "unknown target format: "))]
    UnknownTarget(String),
    #[error("{}", crate::trf!("aucun moteur actif ne sait convertir {from} en {to}", "no active engine can convert {from} to {to}"))]
    NoPath { from: String, to: String },
    #[error("{}", crate::trf!("conversion {from} → {to} impossible pour ce fichier : {}", "{from} → {to} conversion impossible for this file: {}", reasons.join(crate::tr!(" ; ", "; "))))]
    Unavailable { from: String, to: String, reasons: Vec<String> },
    #[error("{}", crate::trf!("option « {key} » : {message}", "option “{key}”: {message}"))]
    InvalidOption { key: String, message: String },
}

/// Une cible proposée à l'utilisateur pour un fichier donné.
#[derive(Clone, Debug, Serialize)]
pub struct TargetChoice {
    pub format: FormatId,
    pub name: String,
    pub category: Category,
    pub best: Plan,
    pub alternatives: Vec<Plan>,
}

pub struct Registry {
    catalog: &'static FormatCatalog,
    engines: Vec<Arc<dyn Engine>>,
    disabled: HashSet<EngineId>,
}

impl Registry {
    pub fn new(catalog: &'static FormatCatalog) -> Self {
        Self { catalog, engines: Vec::new(), disabled: HashSet::new() }
    }

    pub fn catalog(&self) -> &'static FormatCatalog {
        self.catalog
    }

    /// Ajoute un moteur ; remplace un moteur de même identifiant.
    pub fn register(&mut self, engine: Arc<dyn Engine>) {
        self.engines.retain(|e| e.id() != engine.id());
        self.engines.push(engine);
    }

    pub fn set_enabled(&mut self, id: &EngineId, enabled: bool) {
        if enabled {
            self.disabled.remove(id);
        } else {
            self.disabled.insert(id.clone());
        }
    }

    pub fn is_enabled(&self, id: &EngineId) -> bool {
        !self.disabled.contains(id)
    }

    pub fn engines(&self) -> &[Arc<dyn Engine>] {
        &self.engines
    }

    pub fn engine(&self, id: &EngineId) -> Option<&Arc<dyn Engine>> {
        self.engines.iter().find(|e| e.id() == id)
    }

    /// Moteur actif capable d'exécuter cette opération.
    pub fn operation_engine(&self, operation: &str) -> Option<&Arc<dyn Engine>> {
        self.active().find(|e| e.operations().contains(&operation))
    }

    fn active(&self) -> impl Iterator<Item = &Arc<dyn Engine>> {
        self.engines.iter().filter(|e| self.is_enabled(e.id()))
    }

    pub fn planner(&self) -> Planner<'static> {
        let mut p = Planner::new(self.catalog);
        for e in self.active() {
            p.add_engine(e.id(), e.conversions());
        }
        p
    }

    /// Comme `probe`, mais un fichier qu'**aucun** des moteurs capables de lire son format ne
    /// parvient à analyser est signalé comme illisible : endommagé, incomplet, ou pas vraiment
    /// au format que son extension annonce. Sans cela, on proposerait des dizaines de
    /// conversions vouées à l'échec.
    pub fn probe_checked(&self, path: &Path, format: &FormatId) -> Result<Option<SourceInfo>, String> {
        if !self.active().any(|e| !e.enriches_probe() && e.can_probe(format)) {
            // Seuls des moteurs d'analyse savent le lire : leur échec ne prouve rien.
            return Ok(self.probe(path, format));
        }
        self.probe(path, format).map(Some).ok_or_else(|| {
            let name = self.catalog.get(format.as_str()).map_or(format.as_str(), |d| d.label());
            crate::trf!(
                "ce fichier ne peut pas être analysé : il est sans doute endommagé, incomplet, ou n'est pas réellement au format {name}",
                "this file cannot be analyzed: it is probably damaged, incomplete, or not really in the {name} format"
            )
        })
    }

    /// Sonde le fichier avec le premier moteur actif capable de le faire, complétée par les
    /// moteurs d'analyse (voir `Engine::enriches_probe`).
    pub fn probe(&self, path: &Path, format: &FormatId) -> Option<SourceInfo> {
        let mut has_primary = false;
        let mut declined: Option<SourceInfo> = None;
        let mut base = None;
        for e in self.active().filter(|e| !e.enriches_probe() && e.can_probe(format)) {
            has_primary = true;
            match e.probe(path, format) {
                // Variante que ce moteur ne sait pas décoder : rien ne prouve que le fichier est abîmé.
                Some(info) if info.unsupported_by.contains(e.id()) => match &mut declined {
                    Some(d) => d.unsupported_by.extend(info.unsupported_by),
                    None => declined = Some(SourceInfo { features: None, ..info }),
                },
                Some(info) => {
                    base = Some(info);
                    break;
                }
                None => {}
            }
        }
        match (&mut base, declined) {
            (Some(b), Some(d)) => b.unsupported_by.extend(d.unsupported_by),
            (None, Some(d)) => base = Some(d),
            _ => {}
        }
        if has_primary && base.is_none() {
            return None;
        }
        for e in self.active().filter(|e| e.enriches_probe() && e.can_probe(format)) {
            let Some(extra) = e.probe(path, format) else { continue };
            match &mut base {
                Some(b) => merge_probe(b, extra),
                None => {
                    // Sans sonde principale, la liste des caractéristiques reste inconnue.
                    base = Some(SourceInfo { features: None, ..extra });
                }
            }
        }
        base
    }

    /// Premier refus d'un moteur fondé sur le contenu de la source (voir `Engine::content_veto`).
    fn veto(&self, source: &FormatId, info: Option<&SourceInfo>, target: &FormatId) -> Option<String> {
        self.active().find_map(|e| e.content_veto(source, info, target))
    }

    /// Cibles atteignables, triées par catégorie puis nom, avec meilleur plan et alternatives.
    pub fn targets(&self, source: &FormatId, info: Option<&SourceInfo>, options: &OptionValues) -> Vec<TargetChoice> {
        let features = info.and_then(|i| i.features.as_ref());
        let mut out = Vec::new();
        for (target, candidates) in self.planner().plan_all(source, features) {
            if self.veto(source, info, &target).is_some() {
                continue;
            }
            let mut refined: Vec<Plan> = candidates.iter().filter_map(|p| self.refine(p, info, options).ok()).collect();
            if refined.is_empty() {
                continue;
            }
            refined.sort_by(|a, b| a.cost.cmp(&b.cost));
            let best = refined.remove(0);
            let Some(def) = self.catalog.get(target.as_str()) else { continue };
            out.push(TargetChoice { name: def.label().to_owned(), category: def.category, format: target, best, alternatives: refined });
        }
        out.sort_by_key(|a| (a.category, a.name.to_lowercase()));
        out
    }

    /// Meilleur plan pour une cible précise.
    pub fn plan(
        &self,
        source: &FormatId,
        info: Option<&SourceInfo>,
        target: &FormatId,
        options: &OptionValues,
        prefer_engine: Option<&EngineId>,
    ) -> Result<Plan, PlanError> {
        if self.catalog.get(source.as_str()).is_none() {
            return Err(PlanError::UnknownSource(source.to_string()));
        }
        if self.catalog.get(target.as_str()).is_none() {
            return Err(PlanError::UnknownTarget(target.to_string()));
        }
        if let Some(reason) = self.veto(source, info, target) {
            return Err(PlanError::Unavailable { from: source.to_string(), to: target.to_string(), reasons: vec![reason] });
        }
        let features = info.and_then(|i| i.features.as_ref());
        let candidates = self.planner().plan(source, features, target);
        if candidates.is_empty() {
            return Err(PlanError::NoPath { from: source.to_string(), to: target.to_string() });
        }
        let mut reasons = Vec::new();
        let mut refined = Vec::new();
        // Une option invalide pour un chemin (ex. « h264 » sur un détour par WebM) n'écarte que
        // ce chemin ; l'erreur n'est rendue que si aucun chemin n'accepte les options.
        let mut invalid = None;
        for c in &candidates {
            match self.refine(c, info, options) {
                Ok(p) => refined.push(p),
                Err(PlanError::Unavailable { reasons: r, .. }) => reasons.extend(r),
                Err(e @ PlanError::InvalidOption { .. }) => {
                    invalid.get_or_insert(e);
                }
                Err(e) => return Err(e),
            }
        }
        if refined.is_empty() {
            if let Some(e) = invalid {
                return Err(e);
            }
            reasons.dedup();
            return Err(PlanError::Unavailable { from: source.to_string(), to: target.to_string(), reasons });
        }
        // Préférences, dans l'ordre : moteur demandé explicitement, moteurs qui comprennent les
        // options fournies par l'utilisateur (ex. « quality » pour un WebP avec perte), coût.
        refined.sort_by(|a, b| {
            let pa = prefer_engine.is_some_and(|e| a.steps.iter().any(|s| &s.engine == e));
            let pb = prefer_engine.is_some_and(|e| b.steps.iter().any(|s| &s.engine == e));
            pb.cmp(&pa)
                .then_with(|| self.recognized_options(b, options).cmp(&self.recognized_options(a, options)))
                .then_with(|| a.cost.cmp(&b.cost))
        });
        Ok(refined.remove(0))
    }

    /// Nombre d'options fournies par l'utilisateur que les moteurs du plan savent interpréter.
    fn recognized_options(&self, plan: &Plan, options: &OptionValues) -> usize {
        let mut keys: HashSet<String> = HashSet::new();
        for s in &plan.steps {
            if let Some(e) = self.engine(&s.engine) {
                keys.extend(e.options(&s.from, &s.to).into_iter().map(|o| o.key));
            }
        }
        options.keys().filter(|k| keys.contains(*k)).count()
    }

    /// Demande à chaque moteur la fidélité précise de son étape (selon la sonde et les options)
    /// et recalcule le rapport.
    fn refine(&self, plan: &Plan, info: Option<&SourceInfo>, options: &OptionValues) -> Result<Plan, PlanError> {
        let mut plan = plan.clone();
        for (i, step) in plan.steps.iter_mut().enumerate() {
            let Some(engine) = self.engine(&step.engine) else {
                return Err(PlanError::Unavailable {
                    from: plan.source.to_string(),
                    to: plan.target.to_string(),
                    reasons: vec![format!("moteur {} introuvable", step.engine)],
                });
            };
            let specs = engine.options(&step.from, &step.to);
            let resolved = options.resolve(&specs).map_err(|e| PlanError::InvalidOption { key: e.key, message: e.message })?;
            if i == 0 && info.is_some_and(|s| s.unsupported_by.contains(&step.engine)) {
                return Err(PlanError::Unavailable {
                    from: plan.source.to_string(),
                    to: plan.target.to_string(),
                    reasons: vec![crate::trf!(
                        "{} ne sait pas lire cette variante du format",
                        "{} cannot read this variant of the format",
                        engine.info().name
                    )],
                });
            }
            let q = StepQuery { from: &step.from, to: &step.to, source: if i == 0 { info } else { None }, options: &resolved };
            let a = engine.assess(&q);
            if let Some(reason) = a.unavailable {
                return Err(PlanError::Unavailable { from: plan.source.to_string(), to: plan.target.to_string(), reasons: vec![reason] });
            }
            step.fidelity = a.fidelity;
            step.notes = a.notes;
            // Après une étape intermédiaire, le contenu exact n'est pas connu à l'avance : une
            // fidélité « selon le contenu » est comptée avec perte, par prudence. Sans cette règle,
            // un détour optimiste (mkv → mka → mp3) passerait devant la conversion directe.
            if i > 0 && step.fidelity == Fidelity::Depends {
                step.fidelity = Fidelity::Lossy;
                step.notes.push(
                    crate::tr!(
                        "fidélité exacte inconnue avant de produire le fichier intermédiaire : comptée avec perte par prudence",
                        "exact fidelity unknown before the intermediate file exists: counted as lossy to be safe"
                    )
                    .into(),
                );
            }
            if let Some(p) = a.preserves {
                step.preserves = p;
            }
        }
        let src_def = self.catalog.get(plan.source.as_str());
        let (initial, confirmed) = match info.and_then(|i| i.features.as_ref()) {
            Some(f) => (f.clone(), true),
            None => (src_def.map(|d| d.features.clone()).unwrap_or_default(), false),
        };
        let (report, cost, _) = evaluate(self.catalog, &plan.source, &initial, confirmed, &plan.steps);
        plan.report = report;
        plan.cost = cost;
        Ok(plan)
    }
}

/// Ajoute à une sonde ce qu'un moteur d'analyse a établi : lignes de résumé nouvelles,
/// caractéristiques présentes (jamais retirées). Une liste inconnue le reste.
fn merge_probe(base: &mut SourceInfo, extra: SourceInfo) {
    for (k, v) in extra.summary {
        if !base.summary.iter().any(|(bk, bv)| bk == &k || bv == &v) {
            base.summary.push((k, v));
        }
    }
    if let (Some(b), Some(x)) = (base.features.as_mut(), extra.features) {
        b.extend(x);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{ConversionCap, EngineError, EngineInfo, EngineOrigin, ExecContext, StepOutput, StepTask};
    use crate::feature::features;

    /// Moteur factice : `decodes` dit ce que sa sonde répond (`None` : fichier abîmé ;
    /// `Some(false)` : variante qu'il ne sait pas lire ; `Some(true)` : lu).
    struct Fake {
        info: EngineInfo,
        caps: Vec<ConversionCap>,
        decodes: Option<Option<bool>>,
    }

    impl Fake {
        fn engine(id: &str, to: &str, cost: u32, decodes: Option<Option<bool>>) -> Arc<dyn Engine> {
            let info = EngineInfo {
                id: EngineId::new(id),
                name: id.into(),
                version: None,
                license: String::new(),
                homepage: String::new(),
                origin: EngineOrigin::Builtin,
                path: None,
                description: String::new(),
            };
            Arc::new(Fake { info, caps: vec![ConversionCap::new("dds", to, Fidelity::Lossless, &features(&["alpha"]), cost)], decodes })
        }
    }

    impl Engine for Fake {
        fn info(&self) -> &EngineInfo {
            &self.info
        }
        fn conversions(&self) -> &[ConversionCap] {
            &self.caps
        }
        fn can_probe(&self, _format: &FormatId) -> bool {
            self.decodes.is_some()
        }
        fn probe(&self, _path: &Path, _format: &FormatId) -> Option<SourceInfo> {
            match self.decodes? {
                None => None,
                Some(true) => Some(SourceInfo { features: Some(features(&["alpha"])), ..Default::default() }),
                Some(false) => Some(SourceInfo { unsupported_by: vec![self.info.id.clone()], ..Default::default() }),
            }
        }
        fn execute(&self, _task: &StepTask, _ctx: &ExecContext) -> Result<StepOutput, EngineError> {
            unreachable!()
        }
    }

    fn registry(native: Option<bool>) -> Registry {
        let mut r = Registry::new(FormatCatalog::builtin());
        // Le moteur natif, moins cher, sonde le DDS ; ImageMagick (sans sonde) le lit aussi.
        r.register(Fake::engine("image", "png", 1, Some(native)));
        r.register(Fake::engine("imagemagick", "png", 3, None));
        r
    }

    #[test]
    fn an_unreadable_variant_is_not_a_damaged_file() {
        let dds: FormatId = "dds".into();
        let png: FormatId = "png".into();
        let path = Path::new("x.dds");
        // Lu par le moteur natif : il convertit.
        let r = registry(Some(true));
        let info = r.probe_checked(path, &dds).unwrap();
        let t = r.targets(&dds, info.as_ref(), &OptionValues::new());
        assert_eq!(t.iter().find(|c| c.format == png).unwrap().best.steps[0].engine.as_str(), "image");
        // Abîmé : refusé d'emblée.
        assert!(registry(None).probe_checked(path, &dds).is_err());
        // Variante que le moteur natif ne lit pas : pas « abîmé », ImageMagick prend le relais.
        let r = registry(Some(false));
        let info = r.probe_checked(path, &dds).expect("pas abîmé").expect("sonde");
        assert!(info.features.is_none(), "caractéristiques inconnues");
        let t = r.targets(&dds, Some(&info), &OptionValues::new());
        let best = &t.iter().find(|c| c.format == png).expect("PNG proposé").best;
        assert_eq!(best.steps[0].engine.as_str(), "imagemagick");
        assert!(t.iter().all(|c| c.best.steps[0].engine.as_str() != "image"));
    }
}
