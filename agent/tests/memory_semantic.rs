//! Mémoire sémantique : repli sans LM Studio, et vrai rappel avec.
//!
//! ```text
//! .\scripts\with-msvc.ps1 cargo test -p jimmy-agent --test memory_semantic
//! .\scripts\with-msvc.ps1 cargo test -p jimmy-agent --test memory_semantic '--' --ignored
//! ```

use std::sync::{Arc, Mutex};

use jimmy_agent::db::Db;
use jimmy_agent::memory::semantic::SemanticEmbedder;
use jimmy_agent::memory::{MemoryKind, MemoryStore};

const LM_STUDIO: &str = "http://localhost:1234/v1";
const MODELE: &str = "text-embedding-paraphrase-multilingual-minilm-l12-v2.gguf";

fn store(url: &str) -> MemoryStore {
    let db = Arc::new(Mutex::new(Db::open_in_memory().expect("db")));
    MemoryStore::with_semantic(db, SemanticEmbedder::new(url, MODELE))
}

/// LM Studio absent (port fermé) : écrire et rappeler doivent marcher quand
/// même, par hachage, sans erreur ni blocage.
#[tokio::test]
async fn repli_sur_le_hachage_sans_lm_studio() {
    let memoire = store("http://127.0.0.1:9/v1");
    memoire
        .remember_indexed(MemoryKind::Semantic, "Jimmy code son backend en Rust", "test", 0.8)
        .await
        .expect("l'écriture ne dépend pas de LM Studio");
    let debut = std::time::Instant::now();
    let hits = memoire.recall_semantic("backend Rust", 5).await.expect("rappel");
    assert_eq!(hits.len(), 1, "le hachage doit retrouver le souvenir");
    assert_eq!(memoire.reindex_semantic().await, 0);
    assert!(debut.elapsed() < std::time::Duration::from_secs(10));
}

/// Avec LM Studio : un synonyme sans aucun mot commun doit être retrouvé,
/// ce que le hachage ne sait pas faire.
#[tokio::test]
#[ignore = "nécessite LM Studio sur localhost:1234"]
async fn synonyme_retrouve_avec_lm_studio() {
    let memoire = store(LM_STUDIO);
    memoire
        .remember_indexed(MemoryKind::Semantic, "Jimmy possède une voiture électrique", "test", 0.6)
        .await
        .unwrap();
    memoire
        .remember_indexed(MemoryKind::Semantic, "Jimmy préfère les réponses courtes", "test", 0.6)
        .await
        .unwrap();

    // Hachage seul : aucun mot en commun avec « véhicule ».
    let hachage = memoire.recall("quel véhicule conduit-il", 5).unwrap();
    println!("hachage : {:?}", hachage.iter().map(|h| (&h.memory.content, h.score)).collect::<Vec<_>>());

    let hits = memoire.recall_semantic("quel véhicule conduit-il", 5).await.unwrap();
    println!("sémantique : {:?}", hits.iter().map(|h| (&h.memory.content, h.score)).collect::<Vec<_>>());
    assert!(!hits.is_empty(), "LM Studio doit répondre");
    assert!(hits[0].memory.content.contains("voiture"), "le synonyme doit arriver en tête");
}
