//! Vectorisation locale, sans dépendance ni service externe.
//!
//! Le choix est dicté par le PLAN : « mémoire vectorielle locale », avec des
//! priorités **simplicité**, **performance** et **maintenance facile**. On
//! utilise donc un *vectoriseur de hachage* (hashing trick) :
//!
//! 1. le texte est normalisé (minuscules, sans accents) puis découpé en mots ;
//! 2. chaque mot produit aussi ses trigrammes de caractères, ce qui rend la
//!    recherche robuste aux fautes de frappe et aux variantes ;
//! 3. chaque terme est projeté dans un vecteur de 512 dimensions par
//!    hachage, avec un poids `tf` sous-linéaire ;
//! 4. le vecteur est normalisé : la similarité cosinus devient un produit
//!    scalaire, très rapide.
//!
//! Ce n'est pas un modèle sémantique : « voiture » et « véhicule » ne se
//! rapprochent pas. C'est un choix assumé pour la V1 — le classement est
//! combiné à une recherche lexicale FTS5, et le trait [`crate::memory::MemoryStore`]
//! est le seul point à réécrire si un vrai modèle d'embeddings est ajouté.

use std::collections::HashMap;

/// Dimension des vecteurs stockés.
pub const DIM: usize = 512;

/// Mots outils écartés : ils n'apportent aucun signal discriminant.
const STOPWORDS: &[&str] = &[
    "alors", "au", "aux", "avec", "ce", "ces", "dans", "de", "des", "du", "elle", "en", "et", "eux",
    "il", "je", "la", "le", "les", "leur", "lui", "ma", "mais", "me", "même", "mes", "moi", "mon",
    "ne", "nos", "notre", "nous", "on", "ou", "par", "pas", "pour", "qu", "que", "qui", "sa", "se",
    "ses", "son", "sur", "ta", "te", "tes", "toi", "ton", "tu", "un", "une", "vos", "votre", "vous",
    "c", "d", "j", "l", "m", "n", "s", "t", "y", "été", "être", "avoir", "fait",
];

/// Minuscules, sans accents, ponctuation réduite à des espaces.
pub fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        for folded in fold(ch) {
            if folded.is_alphanumeric() {
                out.push(folded.to_ascii_lowercase());
            } else if folded.is_whitespace() {
                out.push(' ');
            } else {
                out.push(' ');
            }
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Dépliage des caractères latins accentués. `to_lowercase` de Rust conserve
/// les combinaisons Unicode ; on les retire explicitement.
fn fold(ch: char) -> Vec<char> {
    let lower: Vec<char> = ch.to_lowercase().collect();
    let mut out = Vec::with_capacity(lower.len());
    for c in lower {
        match c {
            'à' | 'â' | 'ä' | 'á' | 'ã' | 'å' => out.push('a'),
            'ç' => out.push('c'),
            'è' | 'é' | 'ê' | 'ë' => out.push('e'),
            'ì' | 'î' | 'ï' => out.push('i'),
            'ñ' => out.push('n'),
            'ò' | 'ô' | 'ö' | 'ó' | 'õ' => out.push('o'),
            'ù' | 'û' | 'ü' => out.push('u'),
            'ý' | 'ÿ' => out.push('y'),
            'æ' => {
                out.push('a');
                out.push('e');
            }
            'œ' => {
                out.push('o');
                out.push('e');
            }
            c if c.is_ascii() || c.is_numeric() => out.push(c),
            _ => {}
        }
    }
    out
}

fn tokens(text: &str) -> Vec<String> {
    let normalized = normalize(text);
    let mut out: Vec<String> = Vec::new();
    for word in normalized.split(' ') {
        if word.is_empty() || STOPWORDS.contains(&word) {
            continue;
        }
        out.push(word.to_string());
        // Trigrammes : tolerance aux fautes et aux variantes de conjugated.
        let chars: Vec<char> = word.chars().collect();
        if chars.len() >= 4 {
            for i in 0..chars.len() - 2 {
                out.push(chars[i..i + 3].iter().collect());
            }
        }
    }
    out
}

fn hash_term(term: &str) -> usize {
    // FNV-1a : rapide, stable entre versions de Rust, suffisant ici.
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in term.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    // Le bit de signe est utilisé pour pondérer positivement ou négativement :
    // cela réduit les collisions destructives du hachage.
    (hash % (DIM as u64 * 2)) as usize
}

/// Vectorise un texte en `DIM` flottants normalisés.
pub fn embed(text: &str) -> Vec<f32> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for term in tokens(text) {
        *counts.entry(term).or_insert(0) += 1;
    }
    let mut vec = vec![0f32; DIM];
    for (term, count) in counts {
        let weight = 1.0 + (count as f32).ln();
        let index = hash_term(&term);
        if index < DIM {
            vec[index] += weight;
        } else {
            // Index impair : contribution inverse, pour’annuler le bruit.
            vec[index - DIM] -= weight;
        }
    }
    l2_normalize(&mut vec);
    vec
}

fn l2_normalize(vec: &mut [f32]) {
    let norm: f32 = vec.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > f32::EPSILON {
        for v in vec.iter_mut() {
            *v /= norm;
        }
    }
}

/// Similarité cosinus entre deux vecteurs déjà normalisés.
pub fn similarity(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalisation_supprime_accents_et_majuscules() {
        assert_eq!(normalize("Café, DÉJÀ vu !"), "cafe deja vu");
    }

    #[test]
    fn vecteurs_normes() {
        let v = embed("Jimmy analyse ce dossier");
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-5, "norme = {norm}");
    }

    #[test]
    fn textes_proches_ont_un_score_eleve() {
        let a = embed("le projet jimmy utilise rust et tauri");
        let b = embed("le projet jimmy utilise tauri et rust");
        let c = embed("la recette de la tarte aux pommes");
        assert!(similarity(&a, &b) > similarity(&a, &c));
        assert!(similarity(&a, &b) > 0.8);
    }

    #[test]
    fn faute_de_frappe_toleree() {
        let a = embed("configuration du microphone");
        let b = embed("configuraiton du microphone");
        assert!(similarity(&a, &b) > 0.5);
    }
}