//! Tâches de l'agent et passage en arrière-plan.
//!
//! Chaque tour de l'agent (Chat ou voix) est une **tâche** suivie ici. Une
//! tâche qui travaille encore `DETACH_AFTER` après son premier outil passe
//! **en arrière-plan** : Jimmy se rend disponible pour la suite
//! de la conversation et annonce la fin quand elle arrive.
//!
//! Règles (décidées avec l'utilisateur le 7 octobre) :
//! * **une seule** tâche de fond à la fois : une seconde tâche longue reste au
//!   premier plan, comme avant ;
//! * « STOP » (voix, bouton « Arrêter ») n'arrête que le premier plan ; une
//!   tâche de fond s'arrête par son bouton ou par « arrête tout » ;
//! * pendant une tâche de fond, Jimmy répond en parallèle et sait qu'elle
//!   tourne (`prompt_block`).
//!
//! Le registre ne vit qu'en mémoire : un redémarrage de Jimmy perd les tâches
//! en cours (l'historique garde ce qui a été fait).

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::sync::watch;

/// Une tâche qui travaille encore ce temps après son **premier outil** passe
/// en arrière-plan. Compté depuis le premier outil, pas depuis la demande :
/// MiMo met 4 à 40 s par appel, un premier appel lent faisait passer en fond
/// une tâche presque finie (cas mesuré le 7 octobre : 26 s avant l'outil).
/// 45 s et plus de règle « 3e outil » (choix de l'utilisateur, 7 octobre au
/// soir) : Muse Spark appelle 3 ou 4 outils d'un coup, et 17 tâches sur 19
/// partaient en fond 0 à 5 s après leur premier outil, même les légères
/// (30 à 50 s au total).
pub const DETACH_AFTER: Duration = Duration::from_secs(45);
/// L'annonce de fin d'une tâche de fond attend que Jimmy soit libre (ne
/// parle pas, rien au premier plan), au plus ce délai.
pub const ANNOUNCE_WAIT_MAX: Duration = Duration::from_secs(120);
/// Budget d'une tâche passée en fond (choix de l'utilisateur, 7 octobre) :
/// elle ne bloque plus personne et s'arrête d'un clic ; 25 étapes / 10 min
/// coupaient deux fois de suite une vraie tâche avant qu'elle n'agisse.
pub const BACKGROUND_MAX_ITERATIONS: u32 = 60;
pub const BACKGROUND_DEADLINE: Duration = Duration::from_secs(20 * 60);
/// Réponse affichée quand une tâche de fond est arrêtée.
pub const BACKGROUND_STOPPED: &str = "Tâche de fond arrêtée à ta demande.";

/// La tâche doit-elle passer en arrière-plan ? Seule une tâche **outillée**
/// y va : une simple réponse lente (MiMo met parfois 20 s) reste au premier
/// plan, sinon Jimmy dirait « je m'en occupe » puis répondrait aussitôt.
/// `since_first_tool` : temps écoulé depuis son premier outil (`None` : aucun),
/// comparé au seuil (`Tasks::detach_after`, `DETACH_AFTER` par défaut).
/// Jamais pendant une demande d'autorisation : l'utilisateur est en train
/// de répondre à la carte, la tâche n'est pas « partie ».
pub fn should_detach(since_first_tool: Option<Duration>, threshold: Duration, awaiting_approval: bool) -> bool {
    !awaiting_approval && since_first_tool.is_some_and(|elapsed| elapsed >= threshold)
}

/// Étape lisible d'un appel d'outil : son nom et son argument principal
/// (« run_command cargo test », « write_file notify.py »).
pub fn step_label(name: &str, arguments: &serde_json::Value) -> String {
    let main = ["path", "command", "query", "url", "name", "tool"]
        .iter()
        .find_map(|key| arguments.get(*key).and_then(|v| v.as_str()))
        .map(|value| value.split_whitespace().collect::<Vec<_>>().join(" "));
    match main {
        Some(value) if !value.is_empty() => format!("{name} {}", value.chars().take(60).collect::<String>()),
        _ => name.to_string(),
    }
}

/// Ce que l'interface et le prompt savent d'une tâche.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskInfo {
    pub id: String,
    pub session_id: String,
    /// Début de la demande (80 caractères au plus).
    pub title: String,
    /// Dernière étape connue (« write_file notify.py »), vide au départ.
    pub step: String,
    /// La tâche est passée en arrière-plan.
    pub background: bool,
    /// Durée écoulée, en secondes, au moment de la lecture.
    pub elapsed_secs: u64,
}

struct Entry {
    id: String,
    session_id: String,
    title: String,
    step: String,
    background: bool,
    started: Instant,
    /// Arrêt propre à cette tâche (bouton de la tâche, « arrête tout »).
    cancel: watch::Sender<bool>,
}

impl Entry {
    fn info(&self) -> TaskInfo {
        TaskInfo {
            id: self.id.clone(),
            session_id: self.session_id.clone(),
            title: self.title.clone(),
            step: self.step.clone(),
            background: self.background,
            elapsed_secs: self.started.elapsed().as_secs(),
        }
    }
}

/// Registre des tâches en cours.
pub struct Tasks {
    entries: Mutex<Vec<Entry>>,
    /// Seuil de passage en fond (`DETACH_AFTER`) ; abaissé par les tests
    /// pour ne pas attendre 45 s.
    detach_after: Mutex<Duration>,
}

impl Default for Tasks {
    fn default() -> Self {
        Tasks { entries: Mutex::new(Vec::new()), detach_after: Mutex::new(DETACH_AFTER) }
    }
}

impl Tasks {
    /// Seuil de passage en fond, lu par le relais de chaque tâche.
    pub fn detach_after(&self) -> Duration {
        *self.detach_after.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Change le seuil (tests de passage en fond, qui n'attendent pas 45 s).
    pub fn set_detach_after(&self, threshold: Duration) {
        *self.detach_after.lock().unwrap_or_else(|e| e.into_inner()) = threshold;
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Entry>> {
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Enregistre une tâche au premier plan. Renvoie son identifiant et le
    /// signal de son arrêt propre.
    pub fn begin(&self, session_id: &str, request: &str) -> (String, watch::Receiver<bool>) {
        let id = uuid::Uuid::new_v4().to_string();
        let (cancel, receiver) = watch::channel(false);
        let title: String = request.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(80).collect();
        self.lock().push(Entry {
            id: id.clone(),
            session_id: session_id.to_string(),
            title,
            step: String::new(),
            background: false,
            started: Instant::now(),
            cancel,
        });
        (id, receiver)
    }

    /// Retire une tâche finie (ou arrêtée).
    pub fn end(&self, id: &str) {
        self.lock().retain(|e| e.id != id);
    }

    /// Passe la tâche en arrière-plan, si aucune autre n'y est déjà (une
    /// seule à la fois). `false` : elle reste au premier plan.
    pub fn try_detach(&self, id: &str) -> bool {
        let mut entries = self.lock();
        if entries.iter().any(|e| e.background && e.id != id) {
            return false;
        }
        match entries.iter_mut().find(|e| e.id == id) {
            Some(entry) => {
                entry.background = true;
                true
            }
            None => false,
        }
    }

    pub fn is_background(&self, id: &str) -> bool {
        self.lock().iter().any(|e| e.id == id && e.background)
    }

    /// Nombre de tâches en arrière-plan (0 ou 1).
    pub fn background_count(&self) -> usize {
        self.lock().iter().filter(|e| e.background).count()
    }

    /// Note la dernière étape (affichée dans le bandeau du Chat et donnée au
    /// modèle quand on lui demande où en est la tâche).
    pub fn set_step(&self, id: &str, step: &str) {
        if let Some(entry) = self.lock().iter_mut().find(|e| e.id == id) {
            entry.step = step.chars().take(120).collect();
        }
    }

    pub fn list(&self) -> Vec<TaskInfo> {
        self.lock().iter().map(Entry::info).collect()
    }

    pub fn get(&self, id: &str) -> Option<TaskInfo> {
        self.lock().iter().find(|e| e.id == id).map(Entry::info)
    }

    /// Arrête une tâche précise. `false` si elle n'existe plus.
    pub fn cancel(&self, id: &str) -> bool {
        match self.lock().iter().find(|e| e.id == id) {
            Some(entry) => {
                let _ = entry.cancel.send(true);
                true
            }
            None => false,
        }
    }

    /// Arrête toutes les tâches (« arrête tout »). Renvoie leur nombre.
    pub fn cancel_all(&self) -> usize {
        let entries = self.lock();
        for entry in entries.iter() {
            let _ = entry.cancel.send(true);
        }
        entries.len()
    }

    /// Bloc du prompt système : la tâche de fond en cours, pour que Jimmy
    /// réponde en parallèle en le sachant (« où en es-tu ? »), sans refaire
    /// son travail. `None` sans tâche de fond : prompt inchangé.
    pub fn prompt_block(&self, session_id: &str) -> Option<String> {
        let entries = self.lock();
        let task = entries.iter().find(|e| e.background)?;
        let place = if task.session_id == session_id { "dans cette conversation" } else { "dans une autre conversation" };
        let step = if task.step.is_empty() { "pas encore d'étape".to_string() } else { task.step.clone() };
        Some(format!(
            "## Tâche en cours en arrière-plan\n\
             - « {} » ({place}), lancée il y a {}, dernière étape : {step}.\n\
             Elle continue sans toi : réponds à la nouvelle demande normalement. Si on te demande où elle en est, \
             réponds d'après ces lignes. Ne refais pas son travail et ne modifie pas les fichiers qu'elle touche. \
             Pour l'arrêter : bouton « Arrêter » de la tâche dans le Chat, ou « arrête tout » à la voix.",
            task.title,
            human_duration(task.started.elapsed()),
        ))
    }
}

/// « 45 s », « 3 min ».
fn human_duration(duration: Duration) -> String {
    let secs = duration.as_secs();
    if secs < 60 {
        format!("{secs} s")
    } else {
        format!("{} min", secs / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn une_seule_tache_de_fond_a_la_fois() {
        let tasks = Tasks::default();
        let (a, _) = tasks.begin("s1", "écris notify.py");
        let (b, _) = tasks.begin("s2", "analyse le dossier");
        assert!(tasks.try_detach(&a));
        assert!(!tasks.try_detach(&b), "la place est prise : b reste au premier plan");
        assert!(tasks.is_background(&a) && !tasks.is_background(&b));
        tasks.end(&a);
        assert!(tasks.try_detach(&b), "la place s'est libérée");
        assert_eq!(tasks.background_count(), 1);
    }

    #[test]
    fn le_bloc_du_prompt_ne_parle_que_de_la_tache_de_fond() {
        let tasks = Tasks::default();
        let (a, _) = tasks.begin("s1", "écris   notify.py\n et branche-le");
        assert!(tasks.prompt_block("s1").is_none(), "premier plan : rien à dire");
        tasks.try_detach(&a);
        tasks.set_step(&a, "write_file notify.py");
        let block = tasks.prompt_block("s1").unwrap();
        assert!(block.contains("« écris notify.py et branche-le » (dans cette conversation)"), "{block}");
        assert!(block.contains("write_file notify.py"), "{block}");
        assert!(tasks.prompt_block("s2").unwrap().contains("dans une autre conversation"));
    }

    #[test]
    fn seule_une_tache_outillee_passe_en_fond() {
        let long = DETACH_AFTER + Duration::from_secs(1);
        assert!(!should_detach(None, DETACH_AFTER, false), "réponse lente sans outil : premier plan");
        // Plusieurs outils d'un coup ne suffisent plus : seul le temps compte.
        assert!(!should_detach(Some(Duration::from_secs(5)), DETACH_AFTER, false), "tâche légère : premier plan");
        assert!(should_detach(Some(long), DETACH_AFTER, false));
        // Cas réel : premier appel au modèle de 26 s, outil juste parti.
        assert!(!should_detach(Some(Duration::ZERO), DETACH_AFTER, false), "le délai part du premier outil");
        assert!(!should_detach(Some(long), DETACH_AFTER, true), "carte d'autorisation en attente");
        assert_eq!(DETACH_AFTER, Duration::from_secs(45));
    }

    #[test]
    fn etape_lisible() {
        let args = serde_json::json!({ "command": "cargo   test\n--workspace", "timeout": 60 });
        assert_eq!(step_label("run_command", &args), "run_command cargo test --workspace");
        assert_eq!(step_label("list_skills", &serde_json::json!({})), "list_skills");
    }

    #[test]
    fn arret_d_une_tache_ou_de_toutes() {
        let tasks = Tasks::default();
        let (a, rx_a) = tasks.begin("s1", "a");
        let (_b, rx_b) = tasks.begin("s1", "b");
        assert!(tasks.cancel(&a));
        assert!(*rx_a.borrow() && !*rx_b.borrow());
        assert_eq!(tasks.cancel_all(), 2);
        assert!(*rx_b.borrow());
        tasks.end(&a);
        assert!(!tasks.cancel(&a), "tâche finie : plus rien à arrêter");
    }
}
