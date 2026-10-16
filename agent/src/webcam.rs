//! Webcam : voir l'utilisateur **à sa demande seulement** (comme la vision
//! d'écran, décision du 7 octobre).
//!
//! Le modèle n'allume jamais la webcam de lui-même : seuls une phrase
//! explicite (« regarde-moi », « tu me vois ? », « regarde ce que je te
//! montre ») ou l'outil `webcam_capture` (carte d'autorisation) en
//! déclenchent une. L'image part au modèle avec la demande et n'est jamais
//! écrite sur disque ni dans l'historique (`Message::images`, comme
//! `screen.rs`) — seule une note « (Photo webcam jointe.) » reste dans le
//! texte.
//!
//! Anti-abus (demande du 16 octobre) : **1 capture par minute au plus**, et
//! nettoyage automatique — les images ne vivent qu'en mémoire le temps du
//! tour (pas de dossier qui grossit, pas de jetons qui brûlent en boucle).
//! Un second appel dans la minute est refusé avec le délai restant, sans
//! allumer la caméra.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::Engine;

use crate::core::types::Image;
use crate::error::{Error, Result};

/// Largeur maximale envoyée au modèle : visage lisible sans faire exploser
/// le coût en jetons (même budget que `screen::MAX_WIDTH`, un peu plus bas :
/// une webcam n'a pas du texte à lire).
pub const MAX_WIDTH: u32 = 1280;
/// Délai minimal entre deux captures (anti-spam + anti-jetons).
pub const MIN_INTERVAL: Duration = Duration::from_secs(60);

/// Dernière capture réussie (globale au processus) : fait respecter
/// `MIN_INTERVAL` même entre deux conversations.
static LAST_CAPTURE: Mutex<Option<Instant>> = Mutex::new(None);

/// Une caméra vue par `nokhwa::query`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CameraDesc {
    pub index: u32,
    pub name: String,
}

/// Une photo prête à joindre, et de quoi la montrer dans l'interface.
#[derive(Debug, Clone)]
pub struct Capture {
    pub image: Image,
    pub camera: String,
    pub width: u32,
    pub height: u32,
}

/// Secondes restantes avant la prochaine capture autorisée (`None` : libre).
/// Fonction pure, testée sans caméra.
pub fn rate_limited(last: Option<Instant>, now: Instant) -> Option<u64> {
    let last = last?;
    let elapsed = now.duration_since(last);
    if elapsed >= MIN_INTERVAL {
        None
    } else {
        Some((MIN_INTERVAL - elapsed).as_secs().max(1))
    }
}

/// Vérifie le garde-fou 1/minute avant d'allumer la caméra.
pub fn check_rate_limit() -> Result<()> {
    let remaining = LAST_CAPTURE
        .lock()
        .ok()
        .and_then(|guard| rate_limited(*guard, Instant::now()));
    match remaining {
        Some(secs) => Err(Error::Tool(format!(
            "REFUSÉ : 1 photo webcam par minute au plus (rythme anti-jetons) — réessaie dans {secs} s, et seulement si l'utilisateur le redemande."
        ))),
        None => Ok(()),
    }
}

fn note_capture() {
    if let Ok(mut guard) = LAST_CAPTURE.lock() {
        *guard = Some(Instant::now());
    }
}

/// La demande veut-elle que Jimy regarde via la webcam ? Phrase explicite
/// seulement (« regarde-moi », « tu me vois ? », « montre » + objet tenu).
/// « regarde mon écran » ou « regarde le code » ne déclenchent rien (c'est
/// `screen::wants_screen`). Insensible à la casse et aux accents.
pub fn wants_webcam(text: &str) -> bool {
    let t = normalize(text);
    if t.trim().is_empty() {
        return false;
    }
    // L'écran, c'est l'autre outil : pas de double capture.
    if t.contains("ecran") || t.contains("fenetre") || t.contains("capture d ecran") {
        // « montre-moi à l'écran » reste un écran, pas une webcam.
        if !t.contains("webcam") && !t.contains("camera") && !t.contains("montre moi") {
            return false;
        }
    }
    const ME: &[&str] = &[
        "regarde moi",
        "regarde-moi",
        "tu me vois",
        "est ce que tu me vois",
        "me vois tu",
        "vois ma tete",
        "vois mon visage",
        "ma tete",
        "mon visage",
        "qui suis je",
        "decris moi",
        "mets un visage",
        "un visage sur moi",
    ];
    const SHOW: &[&str] = &[
        "regarde ce que je",
        "regarde ce que je te montre",
        "je te montre",
        "montre a la camera",
        "montre a la webcam",
        "tiens ca",
        "vois ce que je tiens",
    ];
    const DEVICE: &[&str] = &["webcam", "camera", "cam"];
    const LOOK: &[&str] = &["regard", "vois", "voir", "montre", "allume", "active", "ouvre"];
    if ME.iter().any(|m| t.contains(m)) {
        return true;
    }
    if SHOW.iter().any(|m| t.contains(m)) {
        return true;
    }
    DEVICE.iter().any(|d| t.contains(d)) && LOOK.iter().any(|l| t.contains(l))
}

fn normalize(text: &str) -> String {
    text.to_lowercase()
        .replace('œ', "oe")
        .chars()
        .map(|c| match c {
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'à' | 'â' => 'a',
            'î' | 'ï' => 'i',
            'ô' => 'o',
            'û' | 'ù' => 'u',
            'ç' => 'c',
            '’' => '\'',
            '-' | '_' => ' ',
            c => c,
        })
        .collect()
}

/// Caméras vues par le système (`nokhwa::query`). Vide = aucune webcam
/// (machine sans caméra, permission OS refusée) : l'appelant le dit au lieu
/// de répondre à l'aveugle.
pub fn list_cameras() -> Vec<CameraDesc> {
    let queried = nokhwa::query(nokhwa::utils::ApiBackend::Auto).unwrap_or_default();
    let mut out: Vec<CameraDesc> = queried
        .iter()
        .enumerate()
        .map(|(i, info)| {
            let index = camera_index_to_u32(info.index()).unwrap_or(i as u32);
            CameraDesc { index, name: info.human_name() }
        })
        .collect();
    out.sort_by_key(|c| c.index);
    out.dedup_by_key(|c| c.index);
    out
}

fn camera_index_to_u32(index: &nokhwa::utils::CameraIndex) -> Option<u32> {
    use nokhwa::utils::CameraIndex;
    match index {
        CameraIndex::Index(n) => Some(*n),
        CameraIndex::String(s) => s.parse::<u32>().ok(),
    }
}

/// Libellé d'une liste de caméras pour le modèle et le journal.
pub fn format_cameras(cameras: &[CameraDesc]) -> String {
    if cameras.is_empty() {
        return "aucune webcam détectée".to_string();
    }
    cameras
        .iter()
        .map(|c| format!("{} : {}", c.index, c.name))
        .collect::<Vec<_>>()
        .join(" · ")
}

/// Prend une photo avec la webcam par défaut (ou `camera`, si précisée).
/// Bloquant (allume la caméra, ouvre le flux, lit une trame, referme) : à
/// appeler via `spawn_blocking`. L'image reste en mémoire : jamais sur disque,
/// jamais dans l'historique — seule une note y figure.
pub fn capture_photo(camera: Option<u32>) -> Result<Capture> {
    check_rate_limit()?;
    let cameras = list_cameras();
    if cameras.is_empty() {
        return Err(Error::Tool(
            "aucune webcam détectée (machine sans caméra ou accès refusé par Windows — Réglages → Confidentialité → Caméra)".into(),
        ));
    }
    let wanted = camera.unwrap_or(0);
    let chosen = cameras
        .iter()
        .find(|c| c.index == wanted)
        .unwrap_or(&cameras[0]);
    let index = chosen.index;
    let name = chosen.name.clone();
    // Appelée depuis `spawn_blocking` par tous les appelants : pas de
    // `block_in_place` ici (panique hors runtime async), juste le flux
    // bloquant ouvert, lu puis refermé par `Drop`.
    let capture = capture_blocking(index)?;
    note_capture();
    log::info!(
        "[webcam] photo prise : « {} » ({}), {}×{}, {} Ko",
        name,
        index,
        capture.width,
        capture.height,
        capture.image.base64.len() * 3 / 4 / 1024
    );
    Ok(Capture { camera: name, ..capture })
}

fn capture_blocking(index: u32) -> Result<Capture> {
    use nokhwa::pixel_format::RgbFormat;
    use nokhwa::utils::{CameraIndex, RequestedFormat, RequestedFormatType};
    let mut camera = nokhwa::Camera::new(
        CameraIndex::Index(index),
        RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestResolution),
    )
    .map_err(|e| Error::Tool(format!("webcam inaccessible (index {index}) : {e}")))?;
    camera
        .open_stream()
        .map_err(|e| Error::Tool(format!("flux webcam impossible : {e}")))?;
    let frame = camera
        .frame()
        .map_err(|e| Error::Tool(format!("lecture d'image impossible : {e}")))?;
    let rgb = frame
        .decode_image::<RgbFormat>()
        .map_err(|e| Error::Tool(format!("décodage d'image impossible : {e}")))?;
    let (w, h) = (rgb.width(), rgb.height());
    if w == 0 || h == 0 {
        return Err(Error::Tool("image webcam vide".into()));
    }
    let rgba = image::DynamicImage::ImageRgb8(rgb).to_rgba8();
    let (jpeg, width, height) = crate::screen::encode_jpeg(&rgba, MAX_WIDTH)?;
    Ok(Capture {
        image: Image {
            media_type: "image/jpeg".into(),
            base64: base64::engine::general_purpose::STANDARD.encode(&jpeg),
            label: "Webcam".into(),
        },
        camera: String::new(),
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn une_demande_explicite_declenche_la_webcam() {
        for text in [
            "Regarde-moi",
            "Jimy, tu me vois ?",
            "Mets un visage sur moi",
            "Regarde ce que je te montre",
            "Allume ta webcam",
            "Montre à la caméra ce composant",
            "Décris-moi",
        ] {
            assert!(wants_webcam(text), "« {text} » doit déclencher la webcam");
        }
        for text in [
            "Regarde mon écran",
            "Regarde le code de l'écran de login",
            "Ouvre une fenêtre PowerShell",
            "Allume ton micro",
            "",
        ] {
            assert!(!wants_webcam(text), "« {text} » ne doit pas déclencher la webcam");
        }
    }

    #[test]
    fn une_seule_capture_par_minute() {
        let now = Instant::now();
        assert_eq!(rate_limited(None, now), None);
        assert_eq!(rate_limited(Some(now - Duration::from_secs(61)), now), None);
        let remaining = rate_limited(Some(now - Duration::from_secs(20)), now).expect("throttle");
        assert!((35..=45).contains(&remaining), "{remaining}");
        assert!(rate_limited(Some(now), now).is_some());
    }

    #[test]
    fn liste_des_cameras_formatee() {
        assert_eq!(format_cameras(&[]), "aucune webcam détectée");
        let cams =
            vec![CameraDesc { index: 0, name: "Integrated Camera".into() }, CameraDesc { index: 1, name: "USB Camera".into() }];
        let texte = format_cameras(&cams);
        assert!(texte.contains("Integrated Camera") && texte.contains("USB Camera"), "{texte}");
    }
}
