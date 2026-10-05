//! Synthèse vocale.
//!
//! Fournisseur : **Fish Audio** via OpenRouter
//! (`POST https://openrouter.ai/api/v1/audio/speech`, corps
//! `{model, input, voice, response_format}`).
//!
//! Le provider est un enum et non un trait : à ce jour Fish Audio est le seul
//! backend, et un enum évite une dépendance supplémentaire. Ajouter une voix
//! ne demande qu'une entrée dans [`TtsVoice::PRESETS`] ; ajouter un fournisseur
//! ne demande qu'une variante.
//!
//! Deux règles apprises à l'usage et documentées dans le projet, reprises ici :
//! les retours à la ligne coupent la lecture, donc les lignes sont jointes par
//! des espaces ; et un seul appel par piste avec la voix imposée, sinon Fish
//! choisit une voix au hasard et le timbre change d'une réponse à l'autre.

use std::time::Duration;

use serde::Deserialize;

pub use crate::config::VoiceInfo;
use crate::error::{Error, Result};

/// Catalogue public des voix Fish Audio (lecture sans clé).
const CATALOG_URL: &str = "https://api.fish.audio/model";
/// Budget d'une recherche dans le catalogue (piège 55 : tout appel borné).
const CATALOG_TIMEOUT: Duration = Duration::from_secs(10);
/// Résultats affichés par recherche.
const CATALOG_PAGE: usize = 20;

/// Voix françaises prédéfinies. La première est la voix par défaut de Jimmy.
/// D'autres voix s'ajoutent depuis le catalogue Fish Audio
/// ([`Tts::search_voices`]) et sont gardées dans `tts.library`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TtsVoice {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
}

impl TtsVoice {
    /// Choisie par l'utilisateur le 4 octobre 2026. Deux voix publiques
    /// portent ce nom sur Fish Audio : celle-ci est la plus utilisée (70 000
    /// lectures, 505 « j'aime »).
    pub const NARRATEUR: TtsVoice = TtsVoice {
        id: "4f2a0684dd0247dda68f339738c780e6",
        label: "Le narrateur",
        description: "Voix d'homme grave, ton de narration — voix par défaut",
    };
    pub const FEMININE: TtsVoice = TtsVoice {
        id: "5567200c7d8341738f0892bbacd3be3c",
        label: "Féminine",
        description: "Calme et posée",
    };
    pub const CLEMENCE: TtsVoice = TtsVoice {
        id: "a288bdc744da4ad194921adad6863175",
        label: "Clémence",
        description: "Douce et claire",
    };

    pub const PRESETS: &'static [TtsVoice] = &[TtsVoice::NARRATEUR, TtsVoice::FEMININE, TtsVoice::CLEMENCE];

    /// La voix sous la forme gardée par la bibliothèque.
    pub fn info(&self) -> VoiceInfo {
        VoiceInfo {
            id: self.id.to_string(),
            label: self.label.to_string(),
            description: self.description.to_string(),
            languages: vec!["fr".into()],
            uses: 0,
        }
    }

    pub fn from_id(id: &str) -> Option<TtsVoice> {
        TtsVoice::PRESETS.iter().copied().find(|v| v.id == id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TtsProvider {
    FishAudio,
}

#[derive(Debug, Clone)]
pub struct Speech {
    /// Audio prêt à être lu : PCM 16 bits si le fournisseur l'accepte, WAV
    /// sinon.
    pub bytes: Vec<u8>,
    /// Fréquence d'échantillonnage de `bytes`.
    pub sample_rate: u32,
    /// Durée estimée, à partir du débit moyen observé.
    pub estimated_ms: u64,
}

/// Réponse d'une synthèse, avant agrégation des morceaux.
struct AudioPayload {
    bytes: Vec<u8>,
    sample_rate: u32,
}

/// Voix utilisables d'une réponse du catalogue : entraînées et publiques.
fn parse_catalog(value: &serde_json::Value) -> Vec<VoiceInfo> {
    let Some(items) = value.get("items").and_then(|i| i.as_array()) else {
        return Vec::new();
    };
    items
        .iter()
        .filter(|m| m.get("state").and_then(|s| s.as_str()).unwrap_or("trained") == "trained")
        .filter_map(|m| {
            let id = m.get("_id")?.as_str()?.to_string();
            let label = m.get("title").and_then(|t| t.as_str()).unwrap_or("").trim().to_string();
            let description: String = m
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(160)
                .collect();
            let languages = m
                .get("languages")
                .and_then(|l| l.as_array())
                .map(|l| l.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                .unwrap_or_default();
            let uses = m.get("task_count").and_then(|n| n.as_u64()).unwrap_or(0);
            Some(VoiceInfo { id, label: if label.is_empty() { "Sans nom".into() } else { label }, description, languages, uses })
        })
        .collect()
}

/// Extrait la fréquence d'un en-tête `audio/pcm;rate=44100;channels=1`.
/// Renvoie 0 si l'en-tête est absent : le lecteur ne rééchantillonne pas.
fn parse_rate(content_type: &str) -> u32 {
    content_type
        .split(';')
        .find_map(|part| {
            let part = part.trim();
            part.strip_prefix("rate=")
                .and_then(|v| v.parse::<u32>().ok())
        })
        .unwrap_or(0)
}

/// Clonable à bas coût (le client HTTP est partagé) : la synthèse phrase par
/// phrase lance un morceau en tâche de fond pendant la lecture du précédent.
#[derive(Clone)]
pub struct Tts {
    http: reqwest::Client,
    provider: TtsProvider,
    endpoint: &'static str,
}

impl Tts {
    pub fn new(provider: TtsProvider) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(180))
            .user_agent("jimmy/0.1 (desktop agent)")
            .build()?;
        Ok(Tts {
            http,
            provider,
            endpoint: "https://openrouter.ai/api/v1/audio/speech",
        })
    }

    pub fn provider(&self) -> TtsProvider {
        self.provider
    }

    /// Cherche des voix dans le catalogue public de Fish Audio, par pertinence
    /// (un tri par popularité faisait passer Clémence avant « Le narrateur »
    /// pour la requête « Le narrateur »). `language` filtre (« fr ») ; vide = toutes.
    pub async fn search_voices(&self, query: &str, language: &str) -> Result<Vec<VoiceInfo>> {
        let mut params = vec![
            ("title", query.trim().to_string()),
            ("page_size", CATALOG_PAGE.to_string()),
        ];
        if !language.is_empty() {
            params.push(("language", language.to_string()));
        }
        let response = self
            .http
            .get(CATALOG_URL)
            .query(&params)
            .timeout(CATALOG_TIMEOUT)
            .send()
            .await
            .map_err(|e| Error::Tts(format!("catalogue Fish Audio injoignable : {e}")))?;
        let status = response.status();
        if !status.is_success() {
            return Err(Error::Tts(format!("catalogue Fish Audio : HTTP {status}")));
        }
        let value: serde_json::Value = response
            .json()
            .await
            .map_err(|e| Error::Tts(format!("catalogue Fish Audio illisible : {e}")))?;
        Ok(parse_catalog(&value))
    }

    /// Synthétise `text`. Le texte est d'abord réécrit pour l'oral : les
    /// retours à la ligne sont remplacés par des espaces, ce qui évite les
    /// coupures dans la lecture.
    pub async fn speak(
        &self,
        api_key: &str,
        model: &str,
        voice: &str,
        text: &str,
        chars_per_minute: f32,
    ) -> Result<Speech> {
        if api_key.trim().is_empty() {
            return Err(Error::Tts("clé OPENROUTER_API_KEY absente".into()));
        }
        let prepared = prepare_for_speech(text);
        if prepared.is_empty() {
            return Ok(Speech {
                bytes: Vec::new(),
                sample_rate: 0,
                estimated_ms: 0,
            });
        }

        // Au-delà de cette taille, le fournisseur refuse : on découpe.
        const MAX_CHARS: usize = 4000;
        if prepared.chars().count() <= MAX_CHARS {
            let audio = self.request_once(api_key, model, voice, &prepared).await?;
            let ms = estimate_ms(&prepared, chars_per_minute);
            return Ok(Speech {
                bytes: audio.bytes,
                sample_rate: audio.sample_rate,
                estimated_ms: ms,
            });
        }

        let mut all = Vec::new();
        let mut sample_rate = 0;
        for chunk in split_sentences(&prepared, MAX_CHARS) {
            let audio = self.request_once(api_key, model, voice, &chunk).await?;
            sample_rate = sample_rate.max(audio.sample_rate);
            all.extend(audio.bytes);
        }
        Ok(Speech {
            bytes: all,
            sample_rate,
            estimated_ms: estimate_ms(&prepared, chars_per_minute),
        })
    }

    /// Un appel de synthèse. Retourne les octets et la fréquence annoncée.
    async fn request_once(
        &self,
        api_key: &str,
        model: &str,
        voice: &str,
        text: &str,
    ) -> Result<AudioPayload> {
        let body = serde_json::json!({
            "model": model,
            "input": text,
            "voice": voice,
            // PCM plutôt que MP3 : le modèle ne produit pas de WAV (400), et le
            // PCM arrive déjà dans le format que la carte son consomme — donc
            // aucun décodeur à embarquer.
            "response_format": "pcm",
        });
        let response = self
            .http
            .post(self.endpoint)
            .bearer_auth(api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Tts(e.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            let raw = response.text().await.unwrap_or_default();
            return Err(Error::Tts(format!("HTTP {status} — {}", sanitize(&raw, 300))));
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        if !content_type.contains("audio") {
            let raw = response.text().await.unwrap_or_default();
            return Err(Error::Tts(format!(
                "réponse non audio ({content_type}) — {}",
                sanitize(&raw, 200)
            )));
        }
        let sample_rate = parse_rate(&content_type);
        let bytes = response
            .bytes()
            .await
            .map_err(|e| Error::Tts(e.to_string()))?;
        Ok(AudioPayload {
            bytes: bytes.to_vec(),
            sample_rate,
        })
    }
}

/// Rend un texte écrit pour l'écran lisible à voix haute.
pub fn prepare_for_speech(text: &str) -> String {
    let text = strip_markdown(text);
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\n' | '\r' | '\t' => out.push(' '),
            c => out.push(c),
        }
    }
    // Double espace : le fournisseur le lit comme une pause.
    let mut cleaned = String::with_capacity(out.len());
    let mut previous_space = false;
    for ch in out.chars() {
        if ch == ' ' {
            if !previous_space {
                cleaned.push(' ');
            }
            previous_space = true;
        } else {
            cleaned.push(ch);
            previous_space = false;
        }
    }
    cleaned.trim().to_string()
}

/// Longueur maximale de ce qui est **dit** à voix haute. L'essentiel est en
/// tête (consigne « Sortie » du prompt) ; au-delà, la voix se tairait pendant
/// des minutes — une analyse d'un tour a déjà été lue en 9 000 caractères.
pub const SPOKEN_MAX_CHARS: usize = 240;

/// La ponctuation à l'indice `i` termine-t-elle vraiment une phrase ? Un point
/// dans un nom de fichier (« todo.md ») ou un nombre (« 1.5 ») n'en termine pas
/// une : sans cette règle, la voix disait « todo. » puis s'arrêtait au milieu.
fn ends_sentence(chars: &[char], i: usize) -> bool {
    match chars[i] {
        '!' | '?' | '…' => true,
        '.' => chars.get(i + 1).map_or(true, |c| c.is_whitespace()),
        _ => false,
    }
}

/// Ne garde que le début utile d'un texte déjà préparé pour la voix, en
/// coupant sur des phrases entières. La première phrase est toujours gardée
/// (même longue, elle est alors coupée au dernier mot) : mieux vaut dire la
/// réponse que rien. Ce qui est retiré reste affiché dans la bulle.
pub fn limit_for_speech(text: &str, max_chars: usize) -> String {
    let mut kept = String::new();
    let mut sentence = String::new();
    let chars: Vec<char> = text.chars().collect();
    for (i, &ch) in chars.iter().enumerate() {
        sentence.push(ch);
        if !ends_sentence(&chars, i) {
            continue;
        }
        let s = sentence.trim();
        if kept.is_empty() {
            kept.push_str(s);
        } else if kept.chars().count() + 1 + s.chars().count() <= max_chars {
            kept.push(' ');
            kept.push_str(s);
        } else {
            // Cette phrase ne tient plus : on s'arrête là.
            sentence.clear();
            break;
        }
        sentence.clear();
    }
    let tail = sentence.trim();
    if !tail.is_empty() && (kept.is_empty() || kept.chars().count() + 1 + tail.chars().count() <= max_chars) {
        if !kept.is_empty() {
            kept.push(' ');
        }
        kept.push_str(tail);
    }
    if kept.chars().count() > max_chars {
        kept = cut_words(&kept, max_chars);
    }
    kept.trim().to_string()
}

/// Coupe au dernier mot entier avant `max_chars` (filet pour une phrase
/// démesurée sans ponctuation).
fn cut_words(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for word in text.split_whitespace() {
        if !out.is_empty() && out.chars().count() + 1 + word.chars().count() > max_chars {
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

/// Découpe un texte en morceaux d'environ `max_chars`, aux fins de phrase
/// (puis aux virgules si une phrase est trop longue). Chaque morceau est
/// synthétisé séparément pour commencer à parler plus tôt.
pub fn split_for_speech(text: &str, max_chars: usize) -> Vec<String> {
    let mut sentences: Vec<String> = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = text.chars().collect();
    for (i, &ch) in chars.iter().enumerate() {
        current.push(ch);
        if ends_sentence(&chars, i) {
            sentences.push(current.trim().to_string());
            current.clear();
        }
    }
    if !current.trim().is_empty() {
        sentences.push(current.trim().to_string());
    }
    // Regroupe les phrases courtes, coupe les phrases trop longues.
    let mut parts: Vec<String> = Vec::new();
    for sentence in sentences.into_iter().filter(|s| !s.is_empty()) {
        let pieces: Vec<String> = if sentence.chars().count() > max_chars {
            sentence.split_inclusive(',').map(|p| p.trim().to_string()).collect()
        } else {
            vec![sentence]
        };
        for piece in pieces {
            let count = parts.len();
            match parts.last_mut() {
                // La toute première phrase reste seule : elle doit partir vite.
                Some(last) if count > 1 && last.chars().count() + piece.chars().count() < max_chars => {
                    last.push(' ');
                    last.push_str(&piece);
                }
                _ => parts.push(piece),
            }
        }
    }
    parts
}

/// Retire ce qui ne se lit pas à voix haute : blocs de code, balises
/// Markdown (`**`, `#`, `` ` ``, puces), liens et URL. Sans cela, la synthèse
/// prononçait « astérisque astérisque » ou des adresses entières.
pub fn strip_markdown(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_code = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            if !in_code {
                out.push_str("(code affiché à l'écran). ");
            }
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        // Titres et puces : on garde le texte, pas le marqueur.
        let line = trimmed
            .trim_start_matches('#')
            .trim_start_matches(|c| c == '-' || c == '*' || c == '>')
            .trim_start();
        out.push_str(line);
        out.push('\n');
    }
    // Liens [texte](url) → texte ; URL nues → « lien ».
    let mut cleaned = String::with_capacity(out.len());
    let chars: Vec<char> = out.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '[' {
            if let Some(close) = chars[i..].iter().position(|&c| c == ']') {
                let close = i + close;
                if chars.get(close + 1) == Some(&'(') {
                    if let Some(end) = chars[close..].iter().position(|&c| c == ')') {
                        cleaned.extend(&chars[i + 1..close]);
                        i = close + end + 1;
                        continue;
                    }
                }
            }
        }
        cleaned.push(chars[i]);
        i += 1;
    }
    let words: Vec<String> = cleaned
        .split(' ')
        .map(|w| {
            if w.starts_with("http://") || w.starts_with("https://") {
                "lien".to_string()
            } else {
                w.replace("**", "").replace("__", "").replace('`', "")
            }
        })
        .collect();
    words.join(" ")
}

/// Estimation de durée : le débit retenu lors des essais est d'environ
/// 1 000 caractères par minute d'audio.
pub fn estimate_ms(text: &str, chars_per_minute: f32) -> u64 {
    let rate = if chars_per_minute <= 0.0 {
        1000.0
    } else {
        chars_per_minute
    };
    let chars = text.chars().count() as f32;
    ((chars / rate) * 60_000.0) as u64
}

/// Découpe aux fins de phrase, pour ne jamais couper une phrase en deux.
fn split_sentences(text: &str, max_chars: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for sentence in split_keep_separator(text) {
        if !current.is_empty() && current.chars().count() + sentence.chars().count() > max_chars {
            chunks.push(std::mem::take(&mut current));
        }
        current.push_str(&sentence);
    }
    if !current.trim().is_empty() {
        chunks.push(current);
    }
    chunks
}

fn split_keep_separator(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        current.push(ch);
        if matches!(ch, '.' | '!' | '?' | '…' | ';' | ':') {
            out.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// Retire toute valeur ressemblant à une clé avant de logger une réponse.
///
/// Le format des clés est `sk-` suivi de caractères sans espace ; on masque
/// à partir de ce préfixe jusqu'au premier séparateur.
fn sanitize(text: &str, max: usize) -> String {
    let mut out = String::with_capacity(text.len().min(max));
    let mut rest = text;
    loop {
        let Some(index) = rest.find("sk-") else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..index]);
        out.push_str("***");
        // On saute le début de la clé : préfixe + tout ce qui suit jusqu'au
        // premier caractère de séparation.
        let after = &rest[index + 3..];
        let end = after
            .find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == ',' || c == '}')
            .unwrap_or(after.len());
        rest = &after[end..];
        if out.len() > max {
            break;
        }
    }
    out.chars().take(max).collect()
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct SpeechResponse {
    #[serde(default)]
    id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_narrateur_est_la_voix_par_defaut() {
        assert_eq!(TtsVoice::PRESETS[0], TtsVoice::NARRATEUR);
        assert_eq!(crate::config::TtsSettings::default().voice, TtsVoice::NARRATEUR.id);
    }

    #[test]
    fn le_catalogue_est_lu_sans_les_voix_non_entrainees() {
        let value = serde_json::json!({ "total": 3, "items": [
            { "_id": "4f2a0684dd0247dda68f339738c780e6", "title": "Le narrateur", "description": "A deep,\n resonant male voice",
              "languages": ["fr"], "task_count": 70528, "state": "trained" },
            { "_id": "aaa", "title": "En cours", "state": "training" },
            { "_id": "bbb", "title": "  ", "task_count": 3 }
        ]});
        let voices = parse_catalog(&value);
        assert_eq!(voices.len(), 2, "{voices:?}");
        assert_eq!(voices[0].label, "Le narrateur");
        assert_eq!(voices[0].description, "A deep, resonant male voice");
        assert_eq!(voices[0].uses, 70528);
        assert_eq!(voices[1].label, "Sans nom");
        assert!(parse_catalog(&serde_json::json!({ "erreur": 1 })).is_empty());
    }

    #[test]
    fn decoupage_par_phrases() {
        let parts = split_for_speech("Bonjour. Il est midi. Il fait beau aujourd'hui. Tout va bien.", 40);
        assert_eq!(parts[0], "Bonjour.", "la première phrase part seule");
        assert!(parts.iter().all(|p| p.chars().count() <= 60), "{parts:?}");
        assert_eq!(parts.join(" "), "Bonjour. Il est midi. Il fait beau aujourd'hui. Tout va bien.");
    }

    #[test]
    fn markdown_non_lu_a_voix_haute() {
        let texte = "## Résumé
**Trois** points :
- un `fichier`
```rust
fn main() {}
```
Voir [la doc](https://exemple.fr) ou https://x.fr/a";
        let oral = prepare_for_speech(texte);
        assert!(!oral.contains('*') && !oral.contains('#') && !oral.contains('`'), "{oral}");
        assert!(!oral.contains("fn main"), "{oral}");
        assert!(oral.contains("code affiché"), "{oral}");
        assert!(oral.contains("la doc") && !oral.contains("https"), "{oral}");
        assert!(oral.starts_with("Résumé Trois points"), "{oral}");
    }

    /// La voix ne dit pas toute la réponse : elle s'arrête sur des phrases
    /// entières, sous le plafond, et laisse le reste à l'écran.
    #[test]
    fn la_voix_s_arrete_aux_premieres_phrases() {
        let long = "J'ai écrit le fichier. Il reste à le tester. Voici un détail \
                    qui ne doit pas être lu et qui continue encore sur plusieurs phrases. Et encore une dernière.";
        let spoken = limit_for_speech(long, 60);
        assert!(spoken.starts_with("J'ai écrit le fichier."), "{spoken}");
        assert!(spoken.chars().count() <= 60, "{spoken}");
        assert!(!spoken.contains("ne doit pas être lu"), "{spoken}");
    }

    /// Une première phrase démesurée (sans ponctuation) est coupée au dernier
    /// mot entier, jamais au milieu d'un mot.
    #[test]
    fn une_premiere_phrase_longue_est_coupee_au_mot() {
        let long = "voici une phrase sans ponctuation qui continue longtemps et qui doit être coupée sans casser un mot";
        let spoken = limit_for_speech(long, 40);
        assert!(spoken.chars().count() <= 40, "{spoken}");
        // Ce qui est dit reste les premiers mots entiers de la phrase.
        assert!(
            long.split_whitespace().take(spoken.split_whitespace().count()).eq(spoken.split_whitespace()),
            "{spoken}"
        );
    }

    #[test]
    fn un_texte_court_traverse_la_borne_intact() {
        assert_eq!(limit_for_speech("Bonjour. Ça va ?", 240), "Bonjour. Ça va ?");
    }

    /// Un point dans un nom de fichier ou un nombre ne finit pas une phrase :
    /// sinon la voix s'arrêtait sur « todo. » et reprenait sur « md ».
    #[test]
    fn un_point_dans_un_nom_de_fichier_ne_coupe_pas_la_phrase() {
        let spoken = limit_for_speech("Le fichier todo.md est prêt. La suite arrive plus loin.", 35);
        assert!(spoken.contains("todo.md est prêt."), "{spoken}");
        // Le découpage en morceaux ne scinde pas non plus la phrase au point.
        let parts = split_for_speech("Le fichier todo.md est prêt. La suite arrive.", 200);
        assert_eq!(
            parts.first().map(String::as_str),
            Some("Le fichier todo.md est prêt."),
            "{parts:?}"
        );
    }

    #[test]
    fn retours_a_la_ligne_deviennent_des_espaces() {
        assert_eq!(prepare_for_speech("Bonjour.\n\nÇa va ?"), "Bonjour. Ça va ?");
    }

    #[test]
    fn estimation_de_duree_coherente() {
        // 1000 caractères ≈ 60 s à 1000 caractères/minute.
        let text = "a".repeat(1000);
        assert_eq!(estimate_ms(&text, 1000.0), 60_000);
    }

    #[test]
    fn texte_long_decoupe_sur_les_phrases() {
        let sentence = "Cette phrase est assez longue pour être comptée. ";
        let text = sentence.repeat(400);
        let chunks = split_sentences(&text, 1000);
        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(chunk.chars().count() <= 1100);
        }
    }

    #[test]
    fn secrets_absentes_des_erreurs() {
        let message = sanitize("échec avec sk-or-v1-abcdefghijklmnop dans le texte", 200);
        assert!(!message.contains("abcdefghijklmnop"));
    }
}