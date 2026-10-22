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

/// Voix prédéfinies selon le moteur TTS actif (`fish-audio`/`openrouter`,
/// `windows`, `openai`, `elevenlabs`).
pub fn presets_for_provider(provider: &str) -> Vec<VoiceInfo> {
    match provider {
        "windows" => windows_voices(),
        "openai" => vec![
            VoiceInfo { id: "alloy".into(), label: "Alloy".into(), description: "OpenAI — neutre et équilibrée".into(), languages: vec!["fr".into(), "en".into()], uses: 0 },
            VoiceInfo { id: "echo".into(), label: "Echo".into(), description: "OpenAI — masculine, posée".into(), languages: vec!["fr".into(), "en".into()], uses: 0 },
            VoiceInfo { id: "nova".into(), label: "Nova".into(), description: "OpenAI — féminine, chaleureuse".into(), languages: vec!["fr".into(), "en".into()], uses: 0 },
            VoiceInfo { id: "onyx".into(), label: "Onyx".into(), description: "OpenAI — masculine, grave".into(), languages: vec!["fr".into(), "en".into()], uses: 0 },
            VoiceInfo { id: "ff_siwis".into(), label: "Siwis (Kokoro FR)".into(), description: "Kokoro local — voix française".into(), languages: vec!["fr".into()], uses: 0 },
            VoiceInfo { id: "af_heart".into(), label: "Heart (Kokoro EN)".into(), description: "Kokoro local — voix anglaise".into(), languages: vec!["en".into()], uses: 0 },
        ],
        "elevenlabs" => vec![
            VoiceInfo { id: "21m00Tcm4TlvDq8ikWAM".into(), label: "Rachel".into(), description: "ElevenLabs — claire et posée".into(), languages: vec!["fr".into(), "en".into()], uses: 0 },
            VoiceInfo { id: "pNInz6obpgDQGcFmaJgB".into(), label: "Adam".into(), description: "ElevenLabs — grave, narration".into(), languages: vec!["fr".into(), "en".into()], uses: 0 },
            VoiceInfo { id: "EXAVITQu4vr4xnSDxMaL".into(), label: "Sarah".into(), description: "ElevenLabs — douce et naturelle".into(), languages: vec!["fr".into(), "en".into()], uses: 0 },
        ],
        _ => TtsVoice::PRESETS.iter().map(|v| v.info()).collect(),
    }
}

/// Voix installées dans Windows (`System.Speech`), lues une seule fois et mises
/// en cache, avec une voix `"default"` toujours présente.
pub fn windows_voices() -> Vec<VoiceInfo> {
    static CACHED: std::sync::OnceLock<Vec<VoiceInfo>> = std::sync::OnceLock::new();
    CACHED
        .get_or_init(|| {
            let mut list = vec![VoiceInfo {
                id: "default".into(),
                label: "Voix Windows par défaut".into(),
                description: "Intégrée au PC — gratuite et hors ligne".into(),
                languages: vec!["fr".into(), "en".into()],
                uses: 0,
            }];
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                let script = "Add-Type -AssemblyName System.Speech; \
                              (New-Object System.Speech.Synthesis.SpeechSynthesizer).GetInstalledVoices() | \
                              ForEach-Object { $i = $_.VoiceInfo; Write-Output \"$($i.Name)|$($i.Culture)|$($i.Description)\" }";
                if let Ok(out) = std::process::Command::new("powershell.exe")
                    .args(["-NoProfile", "-NonInteractive", "-Command", script])
                    .creation_flags(CREATE_NO_WINDOW)
                    .output()
                {
                    for line in String::from_utf8_lossy(&out.stdout).lines() {
                        let mut parts = line.splitn(3, '|');
                        let name = parts.next().unwrap_or("").trim();
                        let culture = parts.next().unwrap_or("").trim();
                        let desc = parts.next().unwrap_or("").trim();
                        if !name.is_empty() {
                            list.push(VoiceInfo {
                                id: name.to_string(),
                                label: name.replace("Microsoft ", "").replace(" Desktop", ""),
                                description: if desc.is_empty() { culture.to_string() } else { format!("{culture} — {desc}") },
                                languages: vec![culture.chars().take(2).collect()],
                                uses: 0,
                            });
                        }
                    }
                }
            }
            list
        })
        .clone()
}

/// Vérifie qu'un identifiant de voix est sûr pour le moteur choisi :
/// - Fish Audio (`fish-audio` / `openrouter`) : 32 caractères hexadécimaux ;
/// - Windows, OpenAI, ElevenLabs : 1 à 80 caractères imprimables sans `/` ni `\`.
pub fn is_valid_voice_id(provider: &str, id: &str) -> bool {
    let id = id.trim();
    if id.is_empty() || id.len() > 80 {
        return false;
    }
    match provider {
        "windows" | "openai" | "elevenlabs" => {
            !id.contains('/')
                && !id.contains('\\')
                && !id.contains("..")
                && id.chars().all(|c| !c.is_control() && c != '"' && c != '\'' && c != ';' && c != '`')
        }
        _ => id.len() == 32 && id.chars().all(|c| c.is_ascii_hexdigit()),
    }
}

/// Convertit un flux WAV (`RIFF...WAVE`) en PCM 16 bits mono petit-boutiste
/// avec sa fréquence d'échantillonnage réelle.
pub fn wav_to_pcm(bytes: &[u8]) -> Result<(Vec<u8>, u32)> {
    let mut reader = hound::WavReader::new(std::io::Cursor::new(bytes))
        .map_err(|e| Error::Tts(format!("WAV illisible : {e}")))?;
    let spec = reader.spec();
    let channels = spec.channels.max(1) as usize;
    let samples: Vec<i16> = reader
        .samples::<i16>()
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| Error::Tts(format!("WAV illisible : {e}")))?;
    let mut pcm = Vec::with_capacity((samples.len() / channels) * 2);
    for frame in samples.chunks(channels) {
        let sum: i32 = frame.iter().map(|&s| i32::from(s)).sum();
        let mono = (sum / channels as i32) as i16;
        pcm.extend_from_slice(&mono.to_le_bytes());
    }
    Ok((pcm, spec.sample_rate))
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
            return Err(Error::Tts("clé OpenRouter absente : saisis-la dans l'onglet Voix".into()));
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

    /// Synthétise `text` selon le moteur configuré dans `tts` :
    /// - `"windows"` : voix locale de Windows (`System.Speech`, hors ligne, sans clé) ;
    /// - `"openai"` : API compatible OpenAI (`/v1/audio/speech`, OpenAI ou Kokoro local) ;
    /// - `"elevenlabs"` : API ElevenLabs (`/v1/text-to-speech/{voice}?output_format=pcm_24000`) ;
    /// - `"fish-audio"` / `"openrouter"` (défaut) : Fish Audio via OpenRouter.
    pub async fn speak_with(
        &self,
        tts: &crate::config::TtsSettings,
        api_key: &str,
        voice_override: Option<&str>,
        text: &str,
    ) -> Result<Speech> {
        let voice = voice_override
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .unwrap_or(tts.voice.as_str());
        match tts.provider.as_str() {
            "windows" => speak_windows(voice, text, tts.chars_per_minute).await,
            "openai" => {
                self.speak_chunks(text, tts.chars_per_minute, |chunk| {
                    self.request_openai(&tts.base_url, api_key, &tts.model, voice, chunk)
                })
                .await
            }
            "elevenlabs" => {
                if api_key.trim().is_empty() {
                    return Err(Error::Tts("clé ElevenLabs absente : saisis-la dans l'onglet Voix".into()));
                }
                self.speak_chunks(text, tts.chars_per_minute, |chunk| {
                    self.request_elevenlabs(api_key, &tts.model, voice, chunk)
                })
                .await
            }
            _ => self.speak(api_key, &tts.model, voice, text, tts.chars_per_minute).await,
        }
    }

    async fn speak_chunks<'a, F, Fut>(&'a self, text: &str, chars_per_minute: f32, mut call: F) -> Result<Speech>
    where
        F: FnMut(String) -> Fut,
        Fut: std::future::Future<Output = Result<AudioPayload>>,
    {
        let prepared = prepare_for_speech(text);
        if prepared.is_empty() {
            return Ok(Speech { bytes: Vec::new(), sample_rate: 0, estimated_ms: 0 });
        }
        const MAX_CHARS: usize = 4000;
        if prepared.chars().count() <= MAX_CHARS {
            let audio = call(prepared.clone()).await?;
            return Ok(Speech {
                bytes: audio.bytes,
                sample_rate: audio.sample_rate,
                estimated_ms: estimate_ms(&prepared, chars_per_minute),
            });
        }
        let mut all = Vec::new();
        let mut sample_rate = 0;
        for chunk in split_sentences(&prepared, MAX_CHARS) {
            let audio = call(chunk).await?;
            sample_rate = sample_rate.max(audio.sample_rate);
            all.extend(audio.bytes);
        }
        Ok(Speech {
            bytes: all,
            sample_rate,
            estimated_ms: estimate_ms(&prepared, chars_per_minute),
        })
    }

    async fn request_openai(
        &self,
        base_url: &str,
        api_key: &str,
        model: &str,
        voice: &str,
        text: String,
    ) -> Result<AudioPayload> {
        let base = if base_url.trim().is_empty() {
            "https://api.openai.com/v1"
        } else {
            base_url.trim().trim_end_matches('/')
        };
        let url = format!("{base}/audio/speech");
        let model = if model.trim().is_empty() || model.starts_with("fish-audio/") {
            "tts-1"
        } else {
            model.trim()
        };
        // Si la voix gardée en config est encore le hex 32 de Fish Audio, repli sur `alloy`.
        let voice = if voice.len() == 32 && voice.chars().all(|c| c.is_ascii_hexdigit()) {
            "alloy"
        } else {
            voice
        };
        let body = serde_json::json!({
            "model": model,
            "input": text,
            "voice": voice,
            "response_format": "pcm",
        });
        let mut req = self.http.post(&url).header("Content-Type", "application/json").json(&body);
        if !api_key.trim().is_empty() {
            req = req.bearer_auth(api_key.trim());
        }
        let response = req.send().await.map_err(|e| Error::Tts(e.to_string()))?;
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
        let bytes = response.bytes().await.map_err(|e| Error::Tts(e.to_string()))?;
        if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE" {
            let (pcm, rate) = wav_to_pcm(&bytes)?;
            return Ok(AudioPayload { bytes: pcm, sample_rate: rate });
        }
        let rate = parse_rate(&content_type);
        Ok(AudioPayload {
            bytes: bytes.to_vec(),
            // Standard PCM d'OpenAI (`/v1/audio/speech` avec `response_format: "pcm"`) et Kokoro : 24 kHz.
            sample_rate: if rate > 0 { rate } else { 24_000 },
        })
    }

    async fn request_elevenlabs(
        &self,
        api_key: &str,
        model: &str,
        voice: &str,
        text: String,
    ) -> Result<AudioPayload> {
        let voice = if voice.trim().is_empty() || (voice.len() == 32 && voice.chars().all(|c| c.is_ascii_hexdigit())) {
            "21m00Tcm4TlvDq8ikWAM"
        } else {
            voice.trim()
        };
        let model = if model.trim().is_empty() || model.starts_with("fish-audio/") {
            "eleven_multilingual_v2"
        } else {
            model.trim()
        };
        let url = format!("https://api.elevenlabs.io/v1/text-to-speech/{voice}?output_format=pcm_24000");
        let body = serde_json::json!({
            "text": text,
            "model_id": model,
        });
        let response = self
            .http
            .post(&url)
            .header("xi-api-key", api_key.trim())
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Tts(e.to_string()))?;
        let status = response.status();
        if !status.is_success() {
            let raw = response.text().await.unwrap_or_default();
            return Err(Error::Tts(format!("ElevenLabs HTTP {status} — {}", sanitize(&raw, 300))));
        }
        let bytes = response.bytes().await.map_err(|e| Error::Tts(e.to_string()))?;
        Ok(AudioPayload {
            bytes: bytes.to_vec(),
            sample_rate: 24_000,
        })
    }
}

/// Synthèse vocale locale par Windows (`System.Speech`, hors ligne, sans clé).
/// Le texte et le nom de voix passent par des variables d'environnement :
/// jamais d'interpolation dans le code PowerShell.
async fn speak_windows(voice: &str, text: &str, chars_per_minute: f32) -> Result<Speech> {
    let prepared = prepare_for_speech(text);
    if prepared.is_empty() {
        return Ok(Speech { bytes: Vec::new(), sample_rate: 0, estimated_ms: 0 });
    }
    let script = "Add-Type -AssemblyName System.Speech; \
                  $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; \
                  $v = $env:JIMMY_TTS_VOICE; \
                  if ($v -and $v -ne 'default') { try { $s.SelectVoice($v) } catch {} }; \
                  $ms = New-Object System.IO.MemoryStream; \
                  $s.SetOutputToWaveStream($ms); \
                  $s.Speak($env:JIMMY_TTS_TEXT); \
                  $b = $ms.ToArray(); \
                  [Console]::OpenStandardOutput().Write($b, 0, $b.Length)";
    let mut cmd = tokio::process::Command::new("powershell.exe");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("JIMMY_TTS_TEXT", &prepared)
        .env(
            "JIMMY_TTS_VOICE",
            if voice.len() == 32 && voice.chars().all(|c| c.is_ascii_hexdigit()) {
                "default"
            } else {
                voice
            },
        )
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        cmd.creation_flags(0x0800_0000);
    }
    let out = tokio::time::timeout(Duration::from_secs(30), cmd.output())
        .await
        .map_err(|_| Error::Tts("synthèse Windows trop lente (> 30 s)".into()))?
        .map_err(|e| Error::Tts(format!("synthèse Windows impossible : {e}")))?;
    if !out.status.success() || out.stdout.is_empty() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(Error::Tts(format!("synthèse Windows en échec : {}", err.trim())));
    }
    let (pcm, sample_rate) = wav_to_pcm(&out.stdout)?;
    Ok(Speech {
        bytes: pcm,
        sample_rate,
        estimated_ms: estimate_ms(&prepared, chars_per_minute),
    })
}

/// Rend un texte écrit pour l'écran lisible à voix haute.
pub fn prepare_for_speech(text: &str) -> String {
    let text = strip_technical(text);
    let text = strip_markdown(&text);
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

/// Longueur de la version courte lue à voix haute (choix « Court » de
/// l'onglet Voix). La borne effective est le réglage `tts.max_spoken_chars`
/// (900 par défaut) : le filtre technique s'applique dans tous les cas, et
/// le reste demeure à l'écran.
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

/// Première phrase complète déjà arrivée dans le flux (`raw` = texte accumulé
/// par le streaming du modèle). Dès qu'une fin de phrase (`. `, `!`, `?`, `…`,
/// ou un retour à la ligne après du texte) est suivie d'au moins un caractère,
/// la première phrase est figée : on peut la nettoyer et lancer son appel TTS
/// pendant que le modèle continue de générer la suite.
///
/// Renvoie `None` tant que la première phrase n'est pas terminée, si un bloc
/// de code ou une balise d'outil est ouvert, ou si la phrase nettoyée est trop
/// courte (`< 6` caractères).
pub fn extract_first_stream_sentence(raw: &str) -> Option<String> {
    if raw.contains("```") || raw.contains("<tool_call>") {
        return None;
    }
    let chars: Vec<char> = raw.chars().collect();
    // Il faut au moins un caractère *après* la ponctuation (ou un `\n`) pour
    // être certain que la phrase est terminée (`todo.` en cours de frappe
    // deviendrait `todo.md` au fragment suivant).
    let mut split_char_idx = None;
    for i in 0..chars.len() {
        let ch = chars[i];
        let boundary = match ch {
            '!' | '?' | '…' => i + 1 < chars.len(),
            '.' => i + 1 < chars.len() && chars[i + 1].is_whitespace(),
            '\n' => i > 0,
            _ => false,
        };
        if boundary {
            split_char_idx = Some(i + 1);
            break;
        }
    }
    let end_idx = split_char_idx?;
    let first_raw: String = chars[..end_idx].iter().collect();
    let cleaned = prepare_for_speech(&first_raw);
    if cleaned.chars().count() < 6 {
        return None;
    }
    // Même découpage que `speak` : le premier morceau de `split_for_speech`.
    split_for_speech(&cleaned, 220).into_iter().next().filter(|s| s.chars().count() >= 6)
}

/// Retire ce qui ne se dit jamais à voix haute mais que le seul Markdown
/// laisse passer : payloads JSON/XML bruts, code inline technique, chemins
/// de fichiers, tableaux ASCII, stacktraces et balises d'outil résiduelles.
/// Contrairement à `strip_markdown`, les fences de code sont reconnus même en
/// milieu de ligne, et leur contenu n'est jamais lu (signalé une fois).
/// Le texte conversationnel passe intact, du début à la fin.
pub fn strip_technical(text: &str) -> String {
    // Balise d'outil résiduelle : tout ce qui suit est un appel, pas du texte.
    let text = match text.find("<tool_call>") {
        Some(at) => &text[..at],
        None => text,
    };
    let mut out = String::new();
    // Segments pairs = prose, impairs = code (le fence ouvrant compte 1).
    for (i, part) in text.split("```").enumerate() {
        if i % 2 == 1 {
            if !part.trim().is_empty() {
                out.push_str("(code affiché à l'écran). ");
            }
            continue;
        }
        for line in part.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            // Payloads JSON bruts (« {"status": "ok"} »).
            if let Some(first) = trimmed.chars().next() {
                if first == '{' || first == '}' || first == '[' || first == ']' {
                    continue;
                }
            }
            // Balises XML brutes (« <parameter=…> »).
            if trimmed.starts_with('<') && trimmed.contains('>') {
                continue;
            }
            // Tableaux ASCII (« | Nom | État | »).
            if trimmed.chars().filter(|&c| c == '|').count() >= 2 {
                continue;
            }
            // Stacktraces («     at charger (src/main.rs:42) »).
            if is_stacktrace_line(trimmed) {
                continue;
            }
            out.push_str(&scrub_inline_code(trimmed));
            out.push('\n');
        }
    }
    out
}

/// Ligne de stacktrace : `at fonction (fichier:ligne)`. Une phrase comme
/// « at home » n'a ni parenthèse ni deux-points et passe.
fn is_stacktrace_line(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("at ") else {
        return false;
    };
    rest.contains('(') && rest.contains(':') && rest.contains(')')
}

/// Retire le contenu des codes inline techniques (« `run_command` »), garde
/// celui des mots simples (« `STOP` »), et masque les chemins de fichiers.
/// Un backtick orphelin passe tel quel (`strip_markdown` le retire ensuite).
fn scrub_inline_code(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(open) = rest.find('`') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('`') {
            Some(close) => {
                let code = &after[..close];
                if is_technical_inline(code) {
                    // Supprimé : jointure propre avec ce qui précède.
                    if !out.is_empty() && !out.ends_with(' ') {
                        out.push(' ');
                    }
                } else {
                    out.push_str(code);
                }
                rest = &after[close + 1..];
            }
            None => {
                out.push('`');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    scrub_paths(&out)
}

/// Un code inline est technique s'il porte un marqueur de code (chemin,
/// appel, opérateur, soulignés), un point collé à du texte (« todo.md »),
/// ou une syntaxe typée (« :: », « -> », « @ »).
fn is_technical_inline(code: &str) -> bool {
    if code.chars().any(|c| "/\\()={}[]<>|;_#*~".contains(c)) {
        return true;
    }
    if code.contains("::") || code.contains("->") || code.contains('@') {
        return true;
    }
    let chars: Vec<char> = code.chars().collect();
    chars.windows(3).any(|w| {
        w[1] == '.'
            && (w[0].is_alphanumeric() || w[0] == '_')
            && (w[2].is_alphanumeric() || w[2] == '_')
    })
}

/// Masque les chemins de fichiers hors code (« agent/src/notes.md »,
/// « C:\Dev\Godot »). Gardés : les expressions courtes sans point
/// (« oui/non », « et/ou »), qui sont de la prose.
fn scrub_paths(text: &str) -> String {
    text.split_whitespace()
        .filter(|w| !is_path_word(w))
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_path_word(word: &str) -> bool {
    // Constructions Markdown (`[texte](url)`) : strip_markdown les convertit
    // (« la doc », « lien ») — ne jamais les amputer ici.
    if word.chars().any(|c| "[]()".contains(c)) {
        return false;
    }
    let trimmed = word.trim_matches(|c: char| {
        !(c.is_alphanumeric() || "/\\.~-_".contains(c))
    });
    if trimmed.contains('\\') {
        return true;
    }
    if !trimmed.contains('/') {
        return false;
    }
    trimmed.contains('.')
        || trimmed.len() > 12
        || trimmed.starts_with("./")
        || trimmed.starts_with("../")
        || trimmed.starts_with('/')
        || trimmed.starts_with('~')
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

    /// Réponse longue mêlant explications, code et payload : la voix lit
    /// tout le naturel (7 phrases) et rien du technique. Reproduit la
    /// plainte du 17 octobre 2026 (« il ne lit qu'une ou deux phrases ») :
    /// avant le filtre renforcé, le JSON brut passait tel quel à la voix.
    #[test]
    fn la_voix_lit_tout_le_naturel_sans_le_technique() {
        let reponse = "J'ai terminé l'analyse du dossier. Il contient trois scripts utiles \
            et un fichier de configuration. Le script principal lit les données puis calcule \
            les totaux par jour. La configuration active la sauvegarde chaque soir à \
            vingt-trois heures. Pour lancer le tout, ouvre un terminal dans le dossier du \
            projet. La commande exacte et le fichier sont affichés à l'écran pour les \
            recopier. Dis-moi si tu veux que je modifie un réglage. \
            ```rust\nfn total(lignes: &[i64]) -> i64 {\n    lignes.iter().sum()\n}\n```\n\
            {\"status\": \"ok\", \"fichiers\": 4}";
        let oral = prepare_for_speech(reponse);
        for phrase in [
            "J'ai terminé l'analyse du dossier",
            "trois scripts utiles",
            "calcule les totaux par jour",
            "vingt-trois heures",
            "Dis-moi si tu veux",
        ] {
            assert!(oral.contains(phrase), "{oral}");
        }
        for interdit in ["fn total", "lignes.iter", "{\"status\"", "\"fichiers\"", "```"] {
            assert!(!oral.contains(interdit), "{oral}");
        }
        // Tient sous la borne Standard (900) : rien n'est coupé.
        assert_eq!(limit_for_speech(&oral, 900), oral);
    }

    /// Le contenu d'un code inline technique n'est pas dit ; un mot simple
    /// entre backticks, si. Avant, seul le marqueur était retiré et
    /// `run_command` était prononcé.
    #[test]
    fn le_code_inline_technique_n_est_pas_dit() {
        let oral = prepare_for_speech("Exécute `run_command` maintenant, ou dis `STOP` pour arrêter.");
        assert!(!oral.contains("run_command"), "{oral}");
        assert!(oral.contains("STOP"), "{oral}");
    }

    /// Chemins, tableaux ASCII et stacktraces : à l'écran, jamais à la voix.
    #[test]
    fn chemins_tableaux_et_stacktrace_ne_sont_pas_dits() {
        let oral = prepare_for_speech(
            "Le rapport est prêt. Détail dans agent/src/notes.md.\n\
            | Nom | État |\n| A | ok |\n\
            L'erreur venait de là :\n    at charger (src/main.rs:42)",
        );
        assert!(!oral.contains("agent/src"), "{oral}");
        assert!(!oral.contains('|'), "{oral}");
        assert!(!oral.contains("at charger"), "{oral}");
        assert!(oral.contains("Le rapport est prêt"), "{oral}");
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
    fn extraction_premiere_phrase_en_streaming() {
        // Tant que le point n'est pas suivi d'un espace, ce peut être un nom
        // de fichier en cours (« todo. » → « todo.md »).
        assert_eq!(extract_first_stream_sentence("Le fichier todo."), None);
        assert_eq!(extract_first_stream_sentence("Le fichier todo.md est"), None);
        assert_eq!(
            extract_first_stream_sentence("Le fichier todo.md est prêt. La suite"),
            Some("Le fichier todo.md est prêt.".to_string())
        );
        // Point d'interrogation ou d'exclamation suivi du début de la suite.
        assert_eq!(
            extract_first_stream_sentence("Salut ! Je vais très bien, et toi"),
            Some("Salut !".to_string())
        );
        // Jamais sur un bloc de code ou une balise d'outil.
        assert_eq!(extract_first_stream_sentence("Voici : ```rust\nfn main()"), None);
        assert_eq!(extract_first_stream_sentence("J'écris. <tool_call>"), None);
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

    #[test]
    fn les_quatre_moteurs_tts_ont_leurs_voix_et_validation() {
        for p in ["fish-audio", "windows", "openai", "elevenlabs"] {
            let voices = presets_for_provider(p);
            assert!(!voices.is_empty(), "moteur {p} sans voix");
            for v in &voices {
                assert!(is_valid_voice_id(p, &v.id), "voix {} refusée pour {p}", v.id);
            }
        }
        assert!(!is_valid_voice_id("fish-audio", "alloy"));
        assert!(is_valid_voice_id("openai", "alloy"));
        assert!(!is_valid_voice_id("openai", "../etc/passwd"));
        assert!(!is_valid_voice_id("windows", "voix; echo secret"));
    }

    #[test]
    fn wav_converti_en_pcm_mono_garde_sa_frequence() {
        let mut cursor = std::io::Cursor::new(Vec::new());
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 22_050,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        {
            let mut writer = hound::WavWriter::new(&mut cursor, spec).unwrap();
            writer.write_sample(1000i16).unwrap();
            writer.write_sample(3000i16).unwrap();
            writer.finalize().unwrap();
        }
        let (pcm, rate) = wav_to_pcm(&cursor.into_inner()).unwrap();
        assert_eq!(rate, 22_050);
        assert_eq!(pcm.len(), 2);
        assert_eq!(i16::from_le_bytes([pcm[0], pcm[1]]), 2000);
    }
}