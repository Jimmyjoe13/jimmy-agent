# Développement

## Mise en route

```powershell
.\scripts\install.ps1     # vérifie et installe ce qui manque
.\scripts\dev.ps1         # lance Jimmy
```

## Le problème du toolchain

Rust cible `x86_64-pc-windows-msvc` : le linker et la bibliothèque C runtime
viennent des **Visual Studio Build Tools**, qui fournissent leur propre
environnement. Sans lui, aucune commande `cargo` ne fonctionne.

C'est pourquoi tous les scripts passent par `scripts/with-msvc.ps1` :

```powershell
.\scripts\with-msvc.ps1 cargo build
.\scripts\with-msvc.ps1 cargo test
.\scripts\with-msvc.ps1 cargo clippy
```

`tauri dev` et `tauri build` aussi, via `scripts/dev.ps1` :

```powershell
.\scripts\dev.ps1                     # dev
.\scripts\dev.ps1 build --bundles nsis
```

## Commandes courantes

| Besoin | Commande |
|---|---|
| Compiler | `.\scripts\with-msvc.ps1 cargo build` |
| Tests unitaires | `.\scripts\with-msvc.ps1 cargo test` |
| Test du chemin heureux | `.\scripts\test-happy.ps1` |
| Interface seule | `cd desktop; npm run dev` |
| Vérifier le projet Godot | `Godot.exe --headless --path godot --quit-after 120` |
| Icônes | `.\scripts\make-icons.ps1` |

## Variables d'environnement

Pour une session de développement :

```powershell
$env:JIMMY_LOG='debug'          # error | warn | info | debug | trace
$env:JIMMY_WHISPER_MODEL='ggml-small-q5_1.bin'
$env:JIMMY_LLM_MODEL='mimo-v2.6-pro'
```

## Modifier l'agent

La boucle est dans `agent/src/core/agent.rs`, le prompt dans
`agent/src/core/prompt.rs`. Après modification :

```powershell
.\scripts\with-msvc.ps1 cargo test -p jimmy-agent
.\scripts\test-happy.ps1
```

Le test du chemin heureux vérifie qu'un agent utilise réellement un outil : c'est
le garde-fou le plus utile contre une régression de l'agentique.

## Ajouter un outil

1. Écrire la structure dans `agent/src/tools/` (ex. `mon_outil.rs`) :

```rust
pub struct MonOutil;

impl Tool for MonOutil {
    fn name(&self) -> &str { "mon_outil" }
    fn description(&self) -> &str { "Ce que fait l'outil, en une phrase." }
    fn parameters(&self) -> serde_json::Value {
        schema(serde_json::json!({ "arg": {"type": "string"} }), &["arg"])
    }
    fn capability(&self) -> Capability { Capability::Read }
    fn call<'a>(&'a self, args: &'a serde_json::Value, ctx: &'a ToolContext)
        -> BoxFuture<'a, Result<String>>
    {
        Box::pin(async move {
            ctx.check(Capability::Read, "ce-que-je-lis")?;
            Ok("résultat".into())
        })
    }
}
```

2. L'enregistrer dans `ToolRegistry::register_defaults`.
3. Tester : l'appel direct de l'outil, puis via le test du chemin heureux.

## Modifier l'avatar

| Élément | Fichier |
|---|---|
| Postures par état | `godot/scripts/jimmy.gd` → `POSES` |
| Géométrie du personnage | `godot/scripts/jimmy.gd` → `_build_*` |
| Cadrage caméra | `godot/scripts/main.gd` → `_build_scene` |
| Bulle, fenêtres, routes | `godot/scripts/main.gd` |

Après modification, vérifier **visuellement** — un projet qui compile peut
afficher un personnage déformé :

```powershell
Start-Process "C:\Dev\Godot\Godot_v4.5.1-stable_win64.exe\Godot_v4.5.1-stable_win64.exe" `
  -ArgumentList '--path','godot'
```

Pour tester un état sans passer par l'agent :

```powershell
curl.exe -X POST http://127.0.0.1:8787/state `
  -H "Content-Type: application/json" -d '{"state":"thinking"}'
```

## Conventions

**Rust.** Les modules portent une documentation d'en-tête expliquant *pourquoi*,
pas *quoi*. Les erreurs utilisateur sont des `Error` lisibles, jamais des
`anyhow!` qui fuient. Tout ce qui franchit un `await` utilise un verrou async.

**TypeScript.** `strict` actif, `noUnusedLocals` actif. Toute surface Rust passe
par `api.ts`. Pas de dépendance sans justification écrite dans le fichier.

**GDScript.** Les constantes en tête de fichier, les types explicites. Les
requêtes HTTP sont robustes aux corps malformés : une entrée invalide ne doit
jamais arrêter l'avatar.

**Sécrets.** Aucun secret en dur, jamais journalisé. `.env` est ignoré par git.
Les réponses d'erreur des fournisseurs passent par `sanitize()` avant d'être
affichées.

## Tests

35 tests unitaires couvrent les parties où une erreur serait silencieuse :
similarité vectorielle, correspondance de permissions, détection du wake word,
découpage des phrases longues, extraction mémoire, analyse des permissions.

```powershell
.\scripts\with-msvc.ps1 cargo test
```

Le test du chemin heureux est ignoré par défaut : il consomme de vrais crédits.

```powershell
.\scripts\test-happy.ps1
```

## Git

`main` + branches `feature/*`. Pas de GitFlow.

```powershell
git switch -c feature/nouvelle-voix
```