//! Vision : capture réelle de la fenêtre active (rien n'est enregistré).
//!
//! `cargo test -p jimmy-agent --test screen -- --ignored --nocapture`

#[test]
#[ignore = "capture l'écran réel ; à lancer explicitement"]
fn la_fenetre_active_est_capturee_hors_de_jimmy() {
    let capture = jimmy_agent::screen::capture_active_window(&[std::process::id()]).expect("capture");
    println!("fenêtre : « {} » ({}), {}×{}", capture.image.label, capture.app, capture.width, capture.height);
    assert_eq!(capture.image.media_type, "image/jpeg");
    assert!(capture.width <= jimmy_agent::screen::MAX_WIDTH);
    assert!(capture.image.base64.len() > 1000, "image vide");
    assert!(!capture.image.label.is_empty(), "fenêtre sans nom");
}
