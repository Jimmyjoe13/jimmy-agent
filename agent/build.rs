//! Intègre le projet de l'avatar (`godot/`, hors cache `.godot/`) dans le
//! binaire : en mode installé, Jimy le réécrit à chaque lancement de l'avatar
//! (`App::godot_project_dir`), si bien qu'un script modifié par l'utilisateur
//! (skin maison, couleurs) ne survit pas. Génère `godot_files.rs` :
//! `GODOT_FILES: &[(&str, &[u8])]` (chemin relatif avec « / », contenu).

use std::fs;
use std::path::{Path, PathBuf};

fn collect(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name == ".godot" {
            continue;
        }
        if path.is_dir() {
            // Un fichier ajouté dans un sous-dossier change la date du dossier.
            println!("cargo:rerun-if-changed={}", path.display());
            collect(root, &path, out);
        } else {
            out.push(path);
        }
    }
}

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let root = manifest.join("..").join("godot");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut files = Vec::new();
    collect(&root, &root, &mut files);
    let mut code = String::from("pub static GODOT_FILES: &[(&str, &[u8])] = &[\n");
    for path in &files {
        println!("cargo:rerun-if-changed={}", path.display());
        let rel = path.strip_prefix(&root).expect("dans godot/").to_string_lossy().replace('\\', "/");
        let abs = path.canonicalize().expect("chemin").display().to_string();
        // Préfixe \\?\ de canonicalize : include_bytes! l'accepte.
        code.push_str(&format!("    ({rel:?}, include_bytes!({abs:?})),\n"));
    }
    code.push_str("];\n");
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("godot_files.rs");
    fs::write(out, code).expect("écriture de godot_files.rs");
}
