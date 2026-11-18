//! File d'attente de conversions pour les interfaces (application de bureau, menu contextuel) :
//! on peut y ajouter des tâches pendant que d'autres tournent, et annuler une tâche précise ou
//! toutes. Chaque tâche a son propre jeton d'annulation ; la jauge de ressources est partagée.

use crate::{JobError, JobEvent, JobOutcome, JobRunner, JobSpec, ResourceGate};
use chud_core::CancelToken;
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;

pub type JobId = u64;

/// Échec d'une tâche, sous une forme sérialisable : code stable + message lisible.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct JobFailure {
    pub code: String,
    pub message: String,
}

impl From<&JobError> for JobFailure {
    fn from(e: &JobError) -> Self {
        Self { code: e.code().to_owned(), message: e.to_string() }
    }
}

/// Ce que la file signale à l'interface, dans l'ordre où cela se produit pour chaque tâche :
/// `Started`, des `Job`, puis `Done` ou `Failed` (une tâche annulée avant de démarrer ne reçoit
/// que `Failed`).
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum QueueEvent {
    Started { id: JobId },
    Job { id: JobId, event: JobEvent },
    Done { id: JobId, outcome: JobOutcome },
    Failed { id: JobId, failure: JobFailure },
}

pub type QueueListener = Arc<dyn Fn(QueueEvent) + Send + Sync>;

#[derive(Default)]
struct State {
    pending: VecDeque<(JobId, JobSpec)>,
    running: HashMap<JobId, CancelToken>,
    next_id: JobId,
    stop: bool,
}

struct Shared {
    state: Mutex<State>,
    wake: Condvar,
    listener: QueueListener,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

pub struct JobQueue {
    shared: Arc<Shared>,
    runner: Arc<JobRunner>,
    workers: Vec<JoinHandle<()>>,
}

impl JobQueue {
    /// `workers` fils d'exécution ; `capacity` unités de la jauge (voir [`JobRunner::weight`]).
    pub fn new(runner: Arc<JobRunner>, workers: usize, capacity: u32, listener: QueueListener) -> Self {
        let shared = Arc::new(Shared { state: Mutex::new(State::default()), wake: Condvar::new(), listener });
        let gate = Arc::new(ResourceGate::new(capacity.max(1)));
        let workers = (0..workers.max(1))
            .map(|_| {
                let (shared, runner, gate) = (shared.clone(), runner.clone(), gate.clone());
                std::thread::spawn(move || worker(&shared, &runner, &gate))
            })
            .collect();
        Self { shared, runner, workers }
    }

    pub fn runner(&self) -> &Arc<JobRunner> {
        &self.runner
    }

    /// Ajoute une tâche ; elle démarrera dès qu'un fil et assez de ressources seront libres.
    pub fn submit(&self, spec: JobSpec) -> JobId {
        let mut st = self.shared.lock();
        st.next_id += 1;
        let id = st.next_id;
        st.pending.push_back((id, spec));
        drop(st);
        self.shared.wake.notify_one();
        id
    }

    /// Annule une tâche : retirée si elle attend, interrompue (moteur arrêté, dossier de travail
    /// supprimé) si elle tourne. Renvoie `false` si la tâche est inconnue ou déjà terminée.
    pub fn cancel(&self, id: JobId) -> bool {
        let mut st = self.shared.lock();
        if let Some(pos) = st.pending.iter().position(|(i, _)| *i == id) {
            st.pending.remove(pos);
            drop(st);
            (self.shared.listener)(QueueEvent::Failed { id, failure: (&JobError::Cancelled).into() });
            return true;
        }
        match st.running.get(&id) {
            Some(token) => {
                token.cancel();
                true
            }
            None => false,
        }
    }

    pub fn cancel_all(&self) {
        let mut st = self.shared.lock();
        let pending: Vec<JobId> = st.pending.drain(..).map(|(id, _)| id).collect();
        for token in st.running.values() {
            token.cancel();
        }
        drop(st);
        for id in pending {
            (self.shared.listener)(QueueEvent::Failed { id, failure: (&JobError::Cancelled).into() });
        }
    }

    /// (en attente, en cours)
    pub fn counts(&self) -> (usize, usize) {
        let st = self.shared.lock();
        (st.pending.len(), st.running.len())
    }

    pub fn is_idle(&self) -> bool {
        self.counts() == (0, 0)
    }
}

impl Drop for JobQueue {
    fn drop(&mut self) {
        {
            let mut st = self.shared.lock();
            st.stop = true;
            st.pending.clear();
            for token in st.running.values() {
                token.cancel();
            }
        }
        self.shared.wake.notify_all();
        for w in self.workers.drain(..) {
            let _ = w.join();
        }
    }
}

fn worker(shared: &Shared, runner: &JobRunner, gate: &ResourceGate) {
    loop {
        let (id, spec, token) = {
            let mut st = shared.lock();
            loop {
                if st.stop {
                    return;
                }
                if let Some((id, spec)) = st.pending.pop_front() {
                    let token = CancelToken::new();
                    st.running.insert(id, token.clone());
                    break (id, spec, token);
                }
                st = shared.wake.wait(st).unwrap_or_else(|e| e.into_inner());
            }
        };
        let weight = runner.prepare(&spec).map(|p| runner.weight(&p.plan)).unwrap_or(1);
        let result = {
            let _slot = gate.acquire(weight);
            if token.is_cancelled() {
                Err(JobError::Cancelled)
            } else {
                (shared.listener)(QueueEvent::Started { id });
                runner.run(&spec, &token, &|event| (shared.listener)(QueueEvent::Job { id, event }))
            }
        };
        shared.lock().running.remove(&id);
        (shared.listener)(match result {
            Ok(outcome) => QueueEvent::Done { id, outcome },
            Err(e) => QueueEvent::Failed { id, failure: (&e).into() },
        });
    }
}
