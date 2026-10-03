# Dépannage

## Démarrage

### « link.exe not found » ou « error: linker `link.exe` not found »

Les **Visual Studio Build Tools** manquent, ou `cargo` n'a pas été lancé dans
leur environnement.

```powershell
.\scripts\with-msvc.ps1 cargo build
```

Si le composant C++ manque :

```powershell
winget install Microsoft.VisualStudio.2022.BuildTools `
  --override "--quiet --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

Vérification :

```powershell
& "C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe" `
  -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
```

### « Port 1420 already in use » / « Port 8787 already in use »

Une instance précédente tourne encore.

```powershell
Get-NetTCPConnection -LocalPort 1420,8787,8790 -State Listen |
  ForEach-Object { Stop-Process -Id $_.OwningProcess -Force }
Get-Process | Where-Object { $_.ProcessName -match '^jimmy$|Godot|whisper' } |
  ForEach-Object { Stop-Process -Id $_.Id -Force }
```

### « Error deserializing 'plugins.updater' »

Le plugin de mise à jour exige une clé publique de signature. La V1 n'a pas de
source de distribution : le plugin a été retiré. Si vous le réactivez, il vous
faudra une clé `minisign` et un endpoint.

### Fenêtre noire au lieu de l'interface

Le serveur Vite n'a pas démarré. En dev, `tauri dev` le lance lui-même ; si la
fenêtre est noire, relancez :

```powershell
.\scripts\dev.ps1
```

## Avatar

### L'avatar ne démarre pas

```powershell
curl.exe -s http://127.0.0.1:8787/health     # {"ok":true} si Godot tourne
```

Si rien ne répond, vérifiez que Godot est trouvable :

```powershell
$env:JIMMY_GODOT_EXE='C:\Dev\Godot\Godot_v4.5.1-stable_win64.exe\Godot_v4.5.1-stable_win64.exe'
```

> Sur cette machine, le dossier `C:\Dev\Godot\Godot_v4.5.1-stable_win64.exe`
> porte le nom de l'exécutable. C'est inhabituel mais c'est ce que
> `paths::find_godot_exe` détecte.

### Le personnage est coupé ou hors cadre

La caméra est fixe (`main.gd`, `_build_scene`). Pour la régler précisément,
Godot expose `unproject_position` : afficher la position écran du haut et du bas
du personnage, puis ajuster `position` et `v_offset`.

Cadrage actuel : `position = (0, 0.75, 6.60)`, `v_offset = 0.62`, `fov = 34`.
Le personnage occupe environ 37 % de la hauteur de la fenêtre, dans son tiers
inférieur, la bulle vivant au-dessus.

### La fenêtre Godot ne se déplace pas au glisser

`_unhandled_input` distingue clic et glissement par un seuil de 2 px. Le
glissement n'est pris en compte que si la fenêtre a le focus.

### Le personnage n'est pas visible sur le bureau

La fenêtre est *toujours au premier plan* mais pas *au-dessus des fenêtres
 maximization*. Si elle est cachée, envoyez :

```powershell
curl.exe -X POST http://127.0.0.1:8787/position -H "Content-Type: application/json" -d '{"x":100,"y":100}'
```

## Voix

### « aucun microphone détecté »

```powershell
# Windows
Get-Process | Where-Object { $_.ProcessName -match 'Godot|^jimmy$|whisper' } |
  ForEach-Object { Stop-Process -Id $_.Id -Force }
```

Le micro peut être monopolisé par une autre application (réunion visio,
enregistreur). Jimmy perd alors l'appareil au démarrage.

### « whisper-server a démarré mais ne répond pas »

```powershell
.\data\components\whisper\Release\whisper-server.exe `
  -m .\data\components\whisper\models\ggml-base-q5_1.bin `
  --host 127.0.0.1 --port 8178 -l fr --vad --vad-model .\data\components\whisper\models\ggml-silero-v6.2.0.bin -t 6
```

Si rien ne s'affiche : le modèle est introuvable ou corrompu. Rechargez-le :

```powershell
.\scripts\install.ps1 -SkipBuildTools
```

### « transcription : la reconnaissance vocale n'est pas démarrée »

Appelez `voice_start` depuis l'interface (page **Voix** → bouton contextuel), ou
vérifiez que `data/components/whisper/Release/whisper-server.exe` existe.

### « Jimmy ne se réveille pas »

Whisper confond « Jimmy » avec « J'y mise » ou « j'ai mis ». Les deux formes
sont reconnues (`voice::KNOWN_PHRASES`). Si le problème persiste :

1. vérifiez le seuil VAD dans les paramètres (un micro bruyant demande 0,02) ;
2. vérifiez le modèle — `small-q5` reconnaît « Jimmy » sans ambiguïté ;
3. testez hors micro : page **Voix** → « Mot d'activation » → saisissez la
   transcription et voyez si elle est détectée.

### « HTTP 429 » ou « 402 » sur la voix

Le modèle TTS gratuit est saturé, ou la clé n'a plus de crédit. La page
**Voix** affiche la taille de l'audio générée. Réessayez plus tard, ou
basculez le modèle payant dans les paramètres.

### Aucun son n'est produit

Windows peut avoir muet le périphérique de sortie par défaut. Vérifiez dans
Paramètres → Son. Le test « Lire » de la page **Voix » renvoie la taille de
l'audio : si elle est non nulle, la synthèse fonctionne et le problème est
côté lecture.

> **Erreur historique : « format audio non décodable par Jimmy (MP3) ».**
> Corrigée. Fish Audio ne produit pas de WAV (`wav` renvoie 400), et le code
> demandait du MP3 — que Jimmy ne savait pas décoder, faute de décodeur.
> La synthèse demande désormais du **PCM** (`response_format: "pcm"`), déjà
> dans le format que la carte son consomme : aucune dépendance ajoutée.
> Si vous rencontrez encore ce message, le binaire est antérieur à la
> correction — recompilez avec `.\scripts\build.ps1 -Release`.

### « Jimmy ne se réveille pas » — causes réelles rencontrées

Deux causes distinctes, toutes deux corrigées :

1. **L'écoute n'était jamais démarrée.** Les commandes `voice_start` /
   `voice_stop` existaient côté Rust mais n'étaient appelées nulle part dans
   l'interface : le micro n'était jamais ouvert et Whisper jamais chargé. Un
   bouton « Activer l'écoute » existe désormais en tête de la vue **Voix**, et
   l'écoute s'active automatiquement à la fin de l'onboarding.

2. **Le mot court mal transcrit.** Whisper `base` entend « J'y mise » pour
   « Jimmy ». Les trois formes réelles sont reconnues
   (`voice::KNOWN_PHRASES`), et un test unitaire
   (`pas_de_faux_positif_sur_jai`) garantit que « j'ai besoin » ne réveille
   pas Jimmy.

**Test isolé de l'écoute** — démarre Whisper et ouvre le micro, sans passer par
l'interface :

```powershell
.\scripts\with-msvc.ps1 cmd /c "cargo test -p jimmy-agent --test audio -- --ignored --nocapture ecoute"
```

**Test de la synthèse vocale**, de bout en bout jusqu'à la lecture :

```powershell
.\scripts\with-msvc.ps1 cmd /c "cargo test -p jimmy-agent --test audio -- --ignored --nocapture tts"
```

Les deux tests ignorés s'exécutent aussi via `.\scripts\test-happy.ps1`.

## Agent

### « clé OPENCODE_API_KEY absente »

```powershell
Copy-Item .env.example .env
```

Puis remplissez `.env`. La page **Diagnostic** → « Recharger .env » évite de
redémarrer.

### L'agent répond sans utiliser d'outil

Le modèle ne voit pas les outils si `registry.specs()` est vide. Vérifiez la
vue **Diagnostic** : la ligne « Outils » doit compter au moins 14 entrées.

### « permission refusée »

Le modèle a tenté une action interdite. C'est le comportement attendu : le
message est renvoyé au modèle, qui explique ou propose une autre voie. Pour
autoriser, page **Paramètres** → **Permissions**.

### « importer n'est pas un dossier » ou chemins refusés

Les chemins sont limités à `C:\Users\**` et `C:\Dev\**` par défaut. Voir
`permissions.rs` → `AccessRule::default`. Le motif `C:\Users\**` couvre tout
le profil utilisateur.

### La réponse prend 15 secondes

C'est normal quand quatre outils sont enchaînés : chaque appel LLM est un
aller-retour réseau. Le journal d'activité de la page Chat montre le détail
(outil, durée, succès ou échec).

## Mémoire et Synaptiq

### « aucun souvenir pertinent »

La mémoire se construit **au fil des conversations**. Après quelques échanges,
elle se remplit. La page **Mémoire** montre le compteur.

### Synaptiq injoignable

```powershell
curl.exe -s http://127.0.0.1:8000/v1/health
```

Si Synaptiq est arrêté, Jimmy fonctionne normalement : il est un complément.
La page **Diagnostic** affiche l'état.

## Base de données

### « database is locked »

Un autre processus écrit. Jimmy est mono-instance (le plugin
`single-instance` n'en laisse qu'un). Si une instance traîne :

```powershell
Get-Process jimmy -ErrorAction SilentlyContinue | Stop-Process -Force
```

### Repartir de zéro

```powershell
Remove-Item .\data\jimmy.db* -Force
```

Vous perdez l'historique et la mémoire ; la configuration (`config.json`) et
les skills sont conservés.

## Diagnostic

La page **Diagnostic** vérifie l'essentiel :

| Vérification | Signification |
|---|---|
| Clé OpenCode Go | `OPENCODE_API_KEY` présente |
| Clé OpenRouter | `OPENROUTER_API_KEY` présente |
| whisper.cpp | binaire présent |
| Modèle STT | modèle présent |
| Godot | exécutable trouvable |
| Avatar | Godot répond sur 8787 |
| Synaptiq | instance locale joignable |

Le bouton « Chemins » affiche les répertoires réellement utilisés.