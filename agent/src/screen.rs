//! Vision : capture de la fenêtre active, **à la demande de l'utilisateur
//! seulement** (décision du 7 octobre).
//!
//! Le modèle n'a aucun outil de capture : seuls le bouton « Joindre ma
//! fenêtre » du Chat et une phrase explicite (« regarde mon écran »,
//! `wants_screen`) en déclenchent une. L'image part au modèle avec la demande
//! et n'est jamais écrite sur disque ni dans l'historique (`Message::images`).
//!
//! « Fenêtre active » : quand l'utilisateur clique dans le Chat, la fenêtre
//! au premier plan est… Jimmy. On écarte donc les fenêtres de Jimmy et de son
//! avatar, et on prend la première fenêtre restante dans l'ordre d'empilement
//! (celle sur laquelle il travaillait).

use base64::Engine;

use crate::core::types::Image;
use crate::error::{Error, Result};

/// Largeur maximale envoyée au modèle : lisible (texte d'un éditeur) sans
/// faire exploser le coût en jetons.
pub const MAX_WIDTH: u32 = 1600;
/// Qualité JPEG : un écran de code reste net, ~150-300 Ko.
const JPEG_QUALITY: u8 = 80;
/// En dessous, ce n'est pas une fenêtre de travail (bulle, barre, outil).
const MIN_SIDE: u32 = 120;

/// Ce qu'on sait d'une fenêtre pour la choisir.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub pid: u32,
    pub app: String,
    pub title: String,
    /// Rang dans la pile : plus grand = plus haut.
    pub z: i32,
    pub focused: bool,
    pub minimized: bool,
    pub width: u32,
    pub height: u32,
}

/// Une capture prête à joindre, et de quoi la montrer dans l'interface.
#[derive(Debug, Clone)]
pub struct Capture {
    pub image: Image,
    pub app: String,
    pub width: u32,
    pub height: u32,
}

/// Choisit la fenêtre à capturer : hors processus exclus (Jimmy, avatar) et
/// hors Godot, visible et de taille utile ; la fenêtre au premier plan si
/// elle reste, sinon la plus haute dans la pile.
pub fn pick(candidates: &[Candidate], excluded_pids: &[u32]) -> Option<usize> {
    let usable = |c: &Candidate| {
        !excluded_pids.contains(&c.pid)
            && !c.app.to_lowercase().contains("godot")
            && !c.minimized
            && c.width >= MIN_SIDE
            && c.height >= MIN_SIDE
    };
    candidates
        .iter()
        .enumerate()
        .filter(|(_, c)| usable(c))
        .max_by_key(|(_, c)| (c.focused, c.z))
        .map(|(i, _)| i)
}

/// Réduit (au plus `max_width` de large) et encode en JPEG.
pub fn encode_jpeg(rgba: &image::RgbaImage, max_width: u32) -> Result<(Vec<u8>, u32, u32)> {
    let (w, h) = rgba.dimensions();
    let rgb = image::DynamicImage::ImageRgba8(rgba.clone()).to_rgb8();
    let rgb = if w > max_width {
        let height = ((h as u64 * max_width as u64) / w as u64).max(1) as u32;
        image::imageops::resize(&rgb, max_width, height, image::imageops::FilterType::Triangle)
    } else {
        rgb
    };
    let (width, height) = rgb.dimensions();
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, JPEG_QUALITY)
        .encode_image(&rgb)
        .map_err(|e| Error::Tool(format!("encodage de la capture : {e}")))?;
    Ok((bytes, width, height))
}

/// Capture la fenêtre active (bloquant : à appeler via `spawn_blocking`).
pub fn capture_active_window(excluded_pids: &[u32]) -> Result<Capture> {
    let fail = |what: &str, e: xcap::XCapError| Error::Tool(format!("capture d'écran ({what}) : {e}"));
    let windows = xcap::Window::all().map_err(|e| fail("liste des fenêtres", e))?;
    let candidates: Vec<Candidate> = windows
        .iter()
        .map(|w| Candidate {
            pid: w.pid().unwrap_or(0),
            app: w.app_name().unwrap_or_default(),
            title: w.title().unwrap_or_default(),
            z: w.z().unwrap_or(i32::MIN),
            focused: w.is_focused().unwrap_or(false),
            minimized: w.is_minimized().unwrap_or(true),
            width: w.width().unwrap_or(0),
            height: w.height().unwrap_or(0),
        })
        .collect();
    let index = pick(&candidates, excluded_pids)
        .ok_or_else(|| Error::Tool("aucune fenêtre à capturer (tout est réduit ?)".into()))?;
    let chosen = &candidates[index];
    let rgba = windows[index].capture_image().map_err(|e| fail("image", e))?;
    let (jpeg, width, height) = encode_jpeg(&rgba, MAX_WIDTH)?;
    let label = if chosen.title.trim().is_empty() { chosen.app.clone() } else { chosen.title.trim().to_string() };
    log::info!(
        "[vision] fenêtre capturée : « {} » ({}), {width}×{height}, {} Ko",
        label,
        chosen.app,
        jpeg.len() / 1024
    );
    Ok(Capture {
        image: Image {
            media_type: "image/jpeg".into(),
            base64: base64::engine::general_purpose::STANDARD.encode(&jpeg),
            label: label.chars().take(120).collect(),
        },
        app: chosen.app.clone(),
        width,
        height,
    })
}

/// La demande veut-elle que Jimmy regarde l'écran ? Phrase explicite
/// seulement (« regarde mon écran », « tu vois ma fenêtre ? », « vérifie ce
/// que je fais ») : « l'écran de login » ou « regarde le code » ne capturent
/// rien. Insensible à la casse et aux accents.
pub fn wants_screen(text: &str) -> bool {
    let t: String = text
        .to_lowercase()
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
            c => c,
        })
        .collect();
    let mine = ["mon ecran", "ma fenetre", "mes ecrans", "ce que je fais", "ce que j'ai a l'ecran", "ce qu'il y a sur mon ecran"];
    let look = ["regard", "verifi", "vois", "voir", "check", "jette un oeil", "jettes un oeil", "analyse", "lis ", "lire", "capture", "aide-moi avec", "explique"];
    mine.iter().any(|m| t.contains(m)) && look.iter().any(|l| t.contains(l))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fenetre(pid: u32, app: &str, z: i32, focused: bool) -> Candidate {
        Candidate { pid, app: app.into(), title: app.into(), z, focused, minimized: false, width: 1200, height: 800 }
    }

    #[test]
    fn on_ecarte_jimmy_et_son_avatar() {
        let jimmy = 100;
        let fenetres = vec![
            fenetre(jimmy, "jimmy.exe", 10, true), // le Chat, où l'on vient de cliquer
            fenetre(200, "Godot_v4.5.1", 9, false), // l'avatar, toujours au-dessus
            fenetre(300, "Code.exe", 8, false),    // ce sur quoi l'utilisateur travaille
            fenetre(400, "chrome.exe", 7, false),
        ];
        assert_eq!(pick(&fenetres, &[jimmy]), Some(2));
    }

    #[test]
    fn la_fenetre_au_premier_plan_l_emporte_si_elle_reste() {
        let fenetres = vec![fenetre(300, "Code.exe", 8, false), fenetre(400, "chrome.exe", 7, true)];
        assert_eq!(pick(&fenetres, &[]), Some(1));
    }

    #[test]
    fn fenetres_reduites_ou_minuscules_ignorees() {
        let mut reduite = fenetre(300, "Code.exe", 9, false);
        reduite.minimized = true;
        let mut bulle = fenetre(400, "notif.exe", 8, false);
        bulle.height = 40;
        assert_eq!(pick(&[reduite.clone(), bulle.clone()], &[]), None);
        assert_eq!(pick(&[reduite, bulle, fenetre(500, "explorer.exe", 1, false)], &[]), Some(2));
    }

    #[test]
    fn la_capture_est_reduite_et_en_jpeg() {
        let grand = image::RgbaImage::from_pixel(3200, 1800, image::Rgba([200, 30, 30, 255]));
        let (jpeg, w, h) = encode_jpeg(&grand, MAX_WIDTH).unwrap();
        assert_eq!((w, h), (1600, 900));
        assert_eq!(&jpeg[..2], &[0xFF, 0xD8], "en-tête JPEG");
        let petit = image::RgbaImage::from_pixel(800, 600, image::Rgba([0, 0, 0, 255]));
        assert_eq!(encode_jpeg(&petit, MAX_WIDTH).unwrap().1, 800, "pas d'agrandissement");
    }

    #[test]
    fn seule_une_demande_explicite_declenche_la_capture() {
        for text in [
            "Regarde mon écran",
            "Jimmy, tu vois ma fenêtre ?",
            "vérifie mon ecran stp, j'ai une erreur",
            "Jette un œil à mon écran",
            "Explique-moi ce que je fais là",
            "Regarde ce que j'ai à l'écran",
        ] {
            assert!(wants_screen(text), "« {text} » doit capturer");
        }
        for text in [
            "Regarde le code de l'écran de login",
            "Ajoute un bouton en haut de l'écran",
            "Mon écran est cassé, trouve-moi un réparateur",
            "Ouvre une fenêtre PowerShell",
            "",
        ] {
            assert!(!wants_screen(text), "« {text} » ne doit pas capturer");
        }
    }
}
