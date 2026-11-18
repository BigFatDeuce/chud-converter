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
    #[error("format source inconnu : {0}")]
    UnknownSource(String),
    #[error("format cible inconnu : {0}")]
    UnknownTarget(String),
    #[error("aucun moteur actif ne sait convertir {from} en {to}")]
    NoPath { from: String, to: String },
    #[error("conversion {from} → {to} impossible pour ce fichier : {}", reasons.join(" ; "))]
    Unavailable { from: String, to: String, reasons: Vec<String> },
    #[error("option « {key} » : {message}")]
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

    /// Sonde le fichier avec le premier moteur actif capable de le faire.
    pub fn probe(&self, path: &Path, format: &FormatId) -> Option<SourceInfo> {
        self.active().filter(|e| e.can_probe(format)).find_map(|e| e.probe(path, format))
    }

    /// Cibles atteignables, triées par catégorie puis nom, avec meilleur plan et alternatives.
    pub fn targets(&self, source: &FormatId, info: Option<&SourceInfo>, options: &OptionValues) -> Vec<TargetChoice> {
        let features = info.and_then(|i| i.features.as_ref());
        let mut out = Vec::new();
        for (target, candidates) in self.planner().plan_all(source, features) {
            let mut refined: Vec<Plan> = candidates.iter().filter_map(|p| self.refine(p, info, options).ok()).collect();
            if refined.is_empty() {
                continue;
            }
            refined.sort_by(|a, b| a.cost.cmp(&b.cost));
            let best = refined.remove(0);
            let Some(def) = self.catalog.get(target.as_str()) else { continue };
            out.push(TargetChoice { name: def.name.clone(), category: def.category, format: target, best, alternatives: refined });
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
        let features = info.and_then(|i| i.features.as_ref());
        let candidates = self.planner().plan(source, features, target);
        if candidates.is_empty() {
            return Err(PlanError::NoPath { from: source.to_string(), to: target.to_string() });
        }
        let mut reasons = Vec::new();
        let mut refined = Vec::new();
        for c in &candidates {
            match self.refine(c, info, options) {
                Ok(p) => refined.push(p),
                Err(PlanError::Unavailable { reasons: r, .. }) => reasons.extend(r),
                Err(e) => return Err(e),
            }
        }
        if refined.is_empty() {
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
                step.notes
                    .push("fidélité exacte inconnue avant de produire le fichier intermédiaire : comptée avec perte par prudence".into());
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
