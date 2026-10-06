//! Barrière anti-capture : un besoin ponctuel ne mérite pas un skill.
//!
//! La capture (Voyager) se déclenchait après toute trajectoire de ≥ 3 outils :
//! ~15 skills par jour en usage réel, sans qu'aucun besoin ne revienne. Ce
//! module ne retient qu'une chose : les besoins déjà vus, sous forme de
//! mots-clés. Première occurrence → on note et on ne capture pas ; deuxième
//! occurrence (ou plus) → la capture peut avoir lieu.
//!
//! Persistance : `data/skill_demand.json`, même pattern que
//! `growth_state.json` (lecture tolérante, écriture directe). Borné à 300
//! entrées, les plus anciennes sautent : un besoin d'il y a six mois ne doit
//! pas déclencher une capture aujourd'hui.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::SkillStore;

/// Fichier des besoins déjà vus, dans le dossier de données.
const FILE: &str = "skill_demand.json";
/// Au-delà, les entrées les plus anciennes sont oubliées.
const MAX_ENTRIES: usize = 300;
/// Recouvrement minimal pour dire « déjà vu ». Même échelle que
/// `SkillStore::overlap`, seuil plus bas : ici on veut regrouper large
/// (éviter une capture), pas fusionner (éviter un doublon).
const MIN_OVERLAP: f64 = 0.5;
/// …avec au moins trois mots en commun : une requête pauvre (« vérifie le
/// serveur ») ne doit pas valider une autre requête pauvre sur un ou deux
/// mots génériques partagés. Une demande trop courte pour fournir trois mots
/// communs ne déclenche de toute façon jamais un skill utile.
const MIN_COMMON: usize = 3;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    keywords: Vec<String>,
    count: u32,
}

/// Enregistre la demande et dit si ce besoin a déjà été vu (2e occurrence
/// ou plus). `true` = la capture peut avoir lieu ; `false` = première fois,
/// on note et on passe. Ne rate jamais bruyamment : en cas de doute sur la
/// lecture, on note en mémoire neuve (une capture manquée vaut mieux qu'une
/// capture abusive… et la prochaine occurrence la déclenchera).
pub fn note_request(data_dir: &Path, request: &str) -> bool {
    let path = data_dir.join(FILE);
    let mut entries: Vec<Entry> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    let wanted = SkillStore::keywords(request);
    for entry in entries.iter_mut() {
        let common = wanted.iter().filter(|w| entry.keywords.contains(w)).count();
        if common >= MIN_COMMON
            && SkillStore::overlap(&wanted, &entry.keywords) >= MIN_OVERLAP
        {
            entry.count += 1;
            write_entries(&path, &mut entries);
            return true;
        }
    }
    entries.push(Entry { keywords: wanted, count: 1 });
    write_entries(&path, &mut entries);
    false
}

fn write_entries(path: &Path, entries: &mut Vec<Entry>) {
    while entries.len() > MAX_ENTRIES {
        entries.remove(0);
    }
    if let Ok(raw) = serde_json::to_string_pretty(&entries) {
        let _ = std::fs::write(path, raw);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("jimmy-demand-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    use std::path::PathBuf;

    #[test]
    fn seconde_occurrence_autorise() {
        let d = dir();
        assert!(!note_request(&d, "Redémarre le conteneur Docker du serveur qui ne répond plus"));
        assert!(note_request(&d, "Le conteneur Docker du serveur ne répond plus, redémarre-le"));
    }

    #[test]
    fn besoin_different_bloque() {
        let d = dir();
        assert!(!note_request(&d, "Redémarre le conteneur Docker du serveur qui ne répond plus"));
        assert!(!note_request(&d, "Écris un résumé du dossier docs en français"));
    }

    #[test]
    fn requete_pauvre_ne_valide_rien() {
        // Un seul mot générique en commun ne suffit jamais.
        let d = dir();
        assert!(!note_request(&d, "Vérifie le serveur s'il te plaît"));
        assert!(!note_request(&d, "Redémarre le serveur s'il te plaît"));
    }

    #[test]
    fn borne_et_fichier_lisible() {
        let d = dir();
        // Deux mots communs à toutes (« requete », « alpha »), troisième
        // unique : jamais trois en commun, donc 310 entrées distinctes.
        for i in 0..MAX_ENTRIES + 10 {
            assert!(!note_request(&d, &format!("Requete alpha zzkq{i}")));
        }
        let raw = std::fs::read_to_string(d.join(FILE)).unwrap();
        let entries: Vec<Entry> = serde_json::from_str(&raw).unwrap();
        assert_eq!(entries.len(), MAX_ENTRIES);
        // Fichier corrompu : on repart de zéro sans paniquer, première fois.
        std::fs::write(d.join(FILE), "{ invalide").unwrap();
        assert!(!note_request(&d, "Redémarre le conteneur Docker du serveur"));
    }
}
