/**
 * Vue Voix : microphone, reconnaissance, synthèse, mot d'activation.
 *
 * C'est la page qui rend vérifiable la chaîne vocale sans passer par le wake
 * word : on enregistre, on transcrit, on relit. Elle montre aussi ce que
 * comprend le détecteur de « Jimmy », pour diagnosing sans matériel particulier.
 */
import { api } from "../api";
import { Recorder } from "../audio";
import { QUALITY_LABEL, attempt, capitalize, guard, h, mount, toast } from "../ui";
import type { AppContext } from "../context";
import { card, field, toggle, FOLLOW_UP_CHOICES, LANGUAGES } from "./settings";
import { modelsPanel } from "./models";
import { voicesPanel } from "./voices";

export function voiceView(ctx: AppContext): HTMLElement {
  const recorder = new Recorder();
  let meterTimer = 0;

  // ── Écoute permanente : état lu côté Rust, jamais deviné ──────────────────
  const listenState = h("span", { class: "listen-state off" }, "…");
  const listenButton = h("button", { class: "primary", onclick: () => void toggleListening() }, "…");
  const servers = h("div", { class: "note" }, "");
  const heard = h("div", { class: "heard" });

  // Bandeau de phase : à chaque instant, ce que Jimmy attend de toi.
  const phaseText = h("span", { class: "phase-text" }, "…");
  const banner = h("div", { class: "phase-banner idle", role: "status" }, h("span", { class: "phase-dot" }), phaseText);
  let phaseTimer = 0;
  const wake = capitalize(ctx.status.stt.wake_word);

  function showPhase(phase: string, remaining = 0) {
    window.clearInterval(phaseTimer);
    banner.className = `phase-banner ${phase}`;
    const labels: Record<string, string> = {
      idle: `En veille — dis « ${wake} »`,
      capturing: "Je t'entends…",
      transcribing: "Je transcris ce que tu as dit…",
      thinking: "Je réfléchis…",
      speaking: "Je te réponds à voix haute…",
    };
    if (phase === "your_turn" && remaining > 0) {
      const end = Date.now() + remaining;
      const tick = () => {
        const s = Math.max(0, Math.ceil((end - Date.now()) / 1000));
        phaseText.textContent = `À toi — parle, je t'écoute (${s} s)`;
      };
      tick();
      phaseTimer = window.setInterval(tick, 250);
      return;
    }
    phaseText.textContent = labels[phase] ?? phase;
  }
  // Forme d'onde du héros (maquette) : 56 barres dont la hauteur suit le
  // niveau du micro ; au repos (écoute coupée), une ligne basse et grise.
  const BAR_COUNT = 56;
  const bars = Array.from({ length: BAR_COUNT }, (_, i) => {
    const env = Math.sin((Math.PI * i) / (BAR_COUNT - 1));
    // Le centre de l'onde prend l'accent, les bords restent argent.
    return h("span", { class: `wave-bar${env > 0.55 ? " hot" : ""}`, style: `opacity: ${(0.35 + 0.65 * env).toFixed(2)}` });
  });
  const wave = h("div", { class: "voice-wave", "aria-hidden": "true" }, ...bars);
  let wavePhase = 0;
  function drawWave(level: number) {
    wavePhase += 0.9;
    bars.forEach((bar, i) => {
      const env = Math.sin((Math.PI * i) / (BAR_COUNT - 1));
      const shape = 0.55 + 0.45 * Math.sin(i * 1.7 + wavePhase) * Math.cos(i * 0.6 - wavePhase * 0.7);
      // Plancher visible même en silence, amplitude pleine à la voix.
      const amp = 0.18 + 0.82 * level;
      bar.style.height = `${Math.max(4, Math.round(6 + 100 * env * Math.abs(shape) * amp))}px`;
    });
  }
  drawWave(0);

  // Vumètre de l'écoute permanente : la preuve que le micro capte.
  const liveMeter = h("div", { class: "meter active", title: "Niveau du micro" }, h("div", { class: "meter-fill" }));
  const livePoll = window.setInterval(async () => {
    if (!ctx.voice?.running) return;
    const status = await api.voiceStatus().catch(() => null);
    if (!status) return;
    // Échelle perceptive : la parole à distance tourne autour de 0,005-0,02.
    const level = Math.min(1, Math.sqrt(status.level / 0.05));
    (liveMeter.firstElementChild as HTMLElement).style.width = `${level * 100}%`;
    drawWave(level);
  }, 300);
  ctx.onCleanup(() => {
    window.clearInterval(phaseTimer);
    window.clearInterval(livePoll);
    window.clearInterval(meterTimer);
    // Quitter la vue pendant un enregistrement libère le micro de la fenêtre.
    if (recorder.active) void recorder.stop();
  });

  function renderListening() {
    const voice = ctx.voice;
    const running = Boolean(voice?.running);
    listenState.textContent = running ? "active" : "arrêtée";
    listenState.className = `listen-state ${running ? "on" : "off"}`;
    listenButton.textContent = running ? "Couper l'écoute" : "Activer l'écoute";
    listenButton.className = `${running ? "ghost" : "primary"} voice-listen`;
    listenButton.removeAttribute("disabled");
    liveMeter.style.display = running ? "" : "none";
    wave.classList.toggle("off", !running);
    if (!running) drawWave(0);
    banner.style.display = running ? "" : "none";
    if (running && banner.classList.contains("idle")) showPhase("idle");
    if (!voice) {
      servers.textContent = "État de l'écoute indisponible.";
      return;
    }
    const ready = (ok: boolean) => (ok ? "prêt" : running ? "indisponible" : "au repos");
    mount(
      servers,
      "Mot d'éveil : ",
      h("code", {}, voice.wakeModel),
      ` (${ready(voice.wakeReady)}) · commande : `,
      h("code", {}, voice.commandModel || voice.wakeModel),
      ` (${ready(voice.commandReady || (!voice.commandModel && voice.wakeReady))})`,
      voice.deviceRate ? ` · micro à ${voice.deviceRate} Hz` : "",
    );
  }

  async function toggleListening() {
    const running = Boolean(ctx.voice?.running);
    listenButton.setAttribute("disabled", "");
    listenButton.textContent = running ? "Arrêt…" : "Démarrage… (chargement des modèles)";
    const ok = running
      ? await attempt(() => api.voiceStop(), "arrêt de l'écoute")
      : await attempt(() => api.voiceStart(), "démarrage de l'écoute");
    await ctx.refreshStatus();
    renderListening();
    if (ok && !running) toast(`Écoute active — dis « ${capitalize(ctx.status.stt.wake_word)} ».`);
  }

  function emptyHeard() {
    mount(heard, h("p", { class: "hint" }, "Les phrases transcrites par l'écoute s'affichent ici, en direct."));
  }

  // Fil « ce que Jimmy entend » : le retour qui manquait pour savoir si le
  // micro capte, et si le mot d'éveil est reconnu.
  ctx.onEvent((event) => {
    if (event.type === "listen") showPhase(event.phase ?? "idle", event.remaining ?? 0);
    // Après « Oui ? » ou une réponse, Jimmy attend la suite : on le dit clairement.
    if (event.type === "state" && event.state === "listening" && (event.detail === "Oui ?" || event.detail === "À toi")) {
      heard.querySelector(".hint")?.remove();
      heard.prepend(
        h(
          "div",
          { class: "heard-line matched" },
          h("span", { class: "heard-tag" }, "à toi"),
          h("span", {}, event.detail === "À toi" ? "Conversation ouverte — réponds sans dire « Jimmy »." : "Jimmy t'écoute — parle maintenant."),
        ),
      );
      return;
    }
    if (event.type !== "heard") return;
    heard.querySelector(".hint")?.remove();
    heard.prepend(
      h(
        "div",
        { class: `heard-line ${event.matched ? "matched" : ""}` },
        h("span", { class: "heard-tag" }, event.matched ? "réveil" : "entendu"),
        h("span", {}, event.text ?? ""),
      ),
    );
    while (heard.childElementCount > 8) heard.lastElementChild?.remove();
  });

  // ── Test de reconnaissance (micro de la fenêtre) ──────────────────────────
  const meter = h("div", { class: "meter" }, h("div", { class: "meter-fill" }));
  const transcript = h("div", { class: "transcript" }, "Aucune transcription.");
  const recordButton = h("button", { class: "primary", onclick: () => void toggleRecording() }, "Enregistrer");

  async function toggleRecording() {
    if (!recorder.active) {
      const started = await attempt(() => recorder.start(), "micro");
      if (!started) return;
      recordButton.textContent = "Arrêter et transcrire";
      meter.classList.add("active");
      meterTimer = window.setInterval(() => {
        (meter.firstElementChild as HTMLElement).style.width = `${Math.min(1, recorder.getLevel() * 4) * 100}%`;
      }, 60);
      return;
    }
    window.clearInterval(meterTimer);
    meter.classList.remove("active");
    recordButton.setAttribute("disabled", "");
    recordButton.textContent = "Transcription…";
    const result = await recorder.stop();
    if (result.wav.length === 0) {
      transcript.textContent = "Rien n'a été capturé.";
    } else {
      transcript.textContent = "Transcription en cours (le premier essai charge les modèles)…";
      // Les serveurs démarrent à la demande côté Rust : pas besoin d'avoir
      // activé l'écoute permanente.
      const text = await guard(() => api.sttTranscribe(result.wav), "transcription");
      transcript.textContent = text === undefined ? "Transcription impossible." : text || "(silence)";
    }
    recordButton.removeAttribute("disabled");
    recordButton.textContent = "Enregistrer";
  }

  // ── Synthèse vocale ───────────────────────────────────────────────────────
  const ttsInput = h("input", { class: "field", type: "text", value: "Bonjour, je suis Jimmy." }) as HTMLInputElement;
  const ttsButton = h("button", { class: "primary", onclick: () => void speak() }, "Lire");

  async function speak() {
    const text = ttsInput.value.trim();
    if (!text) return;
    ttsButton.setAttribute("disabled", "");
    ttsButton.textContent = "Synthèse…";
    await guard(() => api.ttsPreview(text), "synthèse vocale");
    ttsButton.removeAttribute("disabled");
    ttsButton.textContent = "Lire";
  }

  // ── Mot d'activation ──────────────────────────────────────────────────────
  const wakeInput = h("input", { class: "field", type: "text", value: "J'y mise, analyse ce dossier" }) as HTMLInputElement;
  const wakeResult = h("div", { class: "note" }, "—");

  async function testWake() {
    const result = await guard(
      () => api.wakeWordStrip(wakeInput.value, ctx.status.stt.wake_word),
      "test du mot d'activation",
    );
    if (!result) return;
    wakeResult.textContent = result.matched
      ? `Détecté — commande retenue : « ${result.command || "(aucune)"} »`
      : "Non détecté : cette phrase ne commence pas par le mot d'activation.";
  }

  const deviceList = h("div", { class: "note" }, "Recherche des microphones…");
  void api
    .voiceDevices()
    .then((devices) => {
      deviceList.textContent =
        devices.length > 0 ? `Microphones : ${devices.join(" · ")}` : "Aucun microphone détecté par Windows.";
    })
    .catch(() => {
      deviceList.textContent = "Liste des microphones indisponible.";
    });

  emptyHeard();
  renderListening();
  void ctx.refreshStatus().then(renderListening);

  // Bibliothèque de voix : ici, dans l'onglet Voix, là où on la cherche (elle
  // était enterrée dans les Paramètres, sous la liste des modèles).
  const voices = voicesPanel({ onApplied: () => void ctx.refreshStatus() });

  // ── Réglages voix et écoute (rapatriés des Paramètres) ───────────────────
  // Tout ce qui touche à la voix de Jimmy vit dans cet onglet : modèle vocal
  // LLM, synthèse, STT, mot d'activation. Enregistrement dédié : on repart de
  // la copie fraîche de Rust pour ne rien écraser des autres onglets.
  const voiceModelInput = h("input", { class: "field", type: "text" }) as HTMLInputElement;
  const sttSelect = h("select", { class: "field" }) as HTMLSelectElement;
  const commandSelect = h("select", { class: "field" }) as HTMLSelectElement;
  const languageSelect = h("select", { class: "field" }) as HTMLSelectElement;
  const wakeWordInput = h("input", { class: "field", type: "text" }) as HTMLInputElement;
  const cuesEnabled = h("input", { type: "checkbox" }) as HTMLInputElement;
  const followSelect = h("select", { class: "field" }) as HTMLSelectElement;
  const pauseInput = h("input", { class: "field", type: "number", min: "400", max: "1500", step: "50" }) as HTMLInputElement;
  const debugAudio = h("input", { type: "checkbox" }) as HTMLInputElement;

  async function loadVoiceSettings() {
    const loaded = await guard(() => api.getSettings(), "réglages voix");
    if (!loaded) return;
    voiceModelInput.value = loaded.llm.voice_model ?? "";
    mount(sttSelect);
    for (const model of ctx.status.stt.models) {
      sttSelect.append(h("option", { value: model.id }, `${model.label} — ${model.note}`));
    }
    sttSelect.value = loaded.stt.model;
    mount(commandSelect);
    commandSelect.append(h("option", { value: "" }, "Identique au wake word (un seul serveur)"));
    for (const model of ctx.status.stt.models) {
      commandSelect.append(h("option", { value: model.id }, `${model.label} — ${model.note}`));
    }
    commandSelect.value = loaded.stt.command_model;
    mount(languageSelect);
    for (const [code, label] of LANGUAGES) {
      languageSelect.append(h("option", { value: code }, label));
    }
    languageSelect.value = loaded.stt.language;
    wakeWordInput.value = loaded.stt.wake_word;
    cuesEnabled.checked = loaded.tts.cues;
    mount(followSelect);
    for (const [seconds, label] of FOLLOW_UP_CHOICES) {
      followSelect.append(h("option", { value: String(seconds) }, label));
    }
    // Une valeur personnalisée (fichier de config édité) reste sélectionnable.
    const current = Math.round(loaded.voice.follow_up_ms / 1000);
    if (![...FOLLOW_UP_CHOICES].some(([s]) => s === current)) {
      followSelect.append(h("option", { value: String(current) }, `${current} s`));
    }
    followSelect.value = String(current);
    pauseInput.value = String(loaded.voice.end_of_speech_ms);
    debugAudio.checked = loaded.voice.debug_audio;
  }

  async function persistVoice() {
    const fresh = await guard(() => api.getSettings(), "réglages voix");
    if (!fresh) return;
    fresh.llm.voice_model = voiceModelInput.value.trim();
    fresh.stt.model = sttSelect.value;
    fresh.stt.command_model = commandSelect.value;
    fresh.stt.language = languageSelect.value;
    fresh.stt.wake_word = wakeWordInput.value.trim() || "jimmy";
    fresh.tts.cues = cuesEnabled.checked;
    fresh.voice.follow_up_ms = Number(followSelect.value) * 1000;
    fresh.voice.end_of_speech_ms = Math.min(1500, Math.max(400, Number(pauseInput.value) || 700));
    fresh.voice.debug_audio = debugAudio.checked;
    const before = ctx.status.stt;
    if (!(await attempt(() => api.saveSettings(fresh), "enregistrement"))) return;
    await ctx.refreshStatus();
    const sttChanged =
      before.model !== fresh.stt.model ||
      before.command_model !== fresh.stt.command_model ||
      before.language !== fresh.stt.language;
    toast(
      sttChanged && ctx.voice?.running
        ? "Réglages voix enregistrés — coupe et relance l'écoute pour appliquer les modèles."
        : "Réglages voix enregistrés",
    );
  }

  // Bibliothèque du modèle vocal : choix immédiat côté Rust ; le champ
  // identifiant suit pour rester cohérent.
  const voiceModels = modelsPanel(ctx, {
    onApplied: (role, model) => {
      if (role === "voice") voiceModelInput.value = model;
    },
  }, "voice");

  void loadVoiceSettings();

  return h(
    "section",
    { class: "view" },
    h(
      "div",
      { class: "card voice-hero" },
      h(
        "div",
        { class: "voice-hero-head" },
        h("h3", { class: "kicker" }, "Écoute permanente · mot d'éveil"),
        h("h2", { class: "voice-hero-word" }, "« ", h("em", {}, capitalize(ctx.status.stt.wake_word)), " »"),
        h(
          "p",
          { class: "note" },
          "Jimmy garde le micro ouvert et attend « ",
          h("strong", {}, capitalize(ctx.status.stt.wake_word)),
          " ». La reconnaissance tourne en local : rien n'est envoyé sur le réseau. Si tu la laisses active, l'écoute reprend toute seule au prochain lancement.",
        ),
      ),
      wave,
      h("div", { class: "voice-hero-actions" }, listenButton, listenState),
      banner,
      liveMeter,
      servers,
      h("h4", {}, "Ce que Jimmy entend"),
      heard,
    ),
    card(
      "Modèle vocal",
      h(
        "p",
        { class: "note" },
        "Le modèle qui te répond à voix haute. Il peut venir d'un autre fournisseur que le principal — choisis-le dans la bibliothèque (appliqué tout de suite), ou tape l'identifiant puis « Enregistrer ».",
      ),
      field("Modèle vocal (vide = le même que le principal)", voiceModelInput, "un modèle plus rapide pour les échanges à voix haute"),
      h(
        "div",
        { class: "row" },
        h("button", { class: "ghost", onclick: () => void loadVoiceSettings() }, "Annuler"),
        h("button", { class: "primary", onclick: () => void persistVoice() }, "Enregistrer"),
      ),
      voiceModels,
    ),
    card(
      "Écoute et sons",
      h(
        "p",
        { class: "note" },
        "whisper.cpp tourne en local : l'audio n'est jamais envoyé dans le cloud. La synthèse, elle, passe par Fish Audio via OpenRouter.",
      ),
      field("Modèle du wake word (rapide)", sttSelect),
      field("Modèle de la commande (précis)", commandSelect),
      field("Langue", languageSelect),
      field("Mot d'activation", wakeWordInput, "jimmy"),
      field("Conversation continue (écoute sans redire le nom)", followSelect),
      field("Pause qui termine ta phrase (ms)", pauseInput),
      h(
        "p",
        { class: "note" },
        "Après chaque réponse, Jimmy t'écoute encore quelques secondes sans que tu aies à redire son nom. Une pause plus courte rend les réponses plus vives, mais Jimmy peut te couper si tu hésites.",
      ),
      toggle("Sons d'état (« Oui ? » quand tu dis son nom, « Oups… » en cas d'échec)", cuesEnabled),
      toggle("Garder les 40 derniers extraits audio pour le diagnostic (sur cette machine seulement)", debugAudio),
      h(
        "div",
        { class: "row" },
        h("button", { class: "ghost", onclick: () => void loadVoiceSettings() }, "Annuler"),
        h("button", { class: "primary", onclick: () => void persistVoice() }, "Enregistrer"),
      ),
    ),
    card(
      "Voix de Jimmy",
      h(
        "p",
        { class: "note" },
        "La voix avec laquelle Jimmy te parle (Fish Audio). « Écouter » lit une phrase d'essai ; « Choisir » l'applique tout de suite. Cherche dans le catalogue pour en ajouter d'autres.",
      ),
      voices,
      h("h4", {}, "Lire un texte avec la voix actuelle"),
      h("div", { class: "row" }, ttsInput, ttsButton),
    ),
    card(
      "Tester la reconnaissance",
      h("p", { class: "note" }, "Enregistre une phrase avec le micro de la fenêtre, puis lis ce que Whisper en comprend."),
      meter,
      h(
        "div",
        { class: "row" },
        recordButton,
        h("button", { class: "ghost", onclick: () => (transcript.textContent = "Aucune transcription.") }, "Effacer"),
      ),
      transcript,
      deviceList,
    ),
    card(
      "Mot d'activation",
      h(
        "p",
        { class: "note" },
        "Whisper approxime les mots courts : le détecteur accepte les variantes courantes (« J'y mise »…) et une petite distance d'édition.",
      ),
      h("div", { class: "row" }, wakeInput, h("button", { class: "ghost", onclick: () => void testWake() }, "Tester")),
      wakeResult,
    ),
  );
}

/** Onboarding : la toute première expérience, conversationnelle et vérifiable. */
export function onboardingView(ctx: AppContext, onDone: () => void): HTMLElement {
  let step = 0;
  const container = h("section", { class: "view onboarding" });
  const status = ctx.status;

  const nameInput = h("input", { class: "field", type: "text", placeholder: "Ton prénom" }) as HTMLInputElement;
  const modelSelect = h("select", { class: "field" }) as HTMLSelectElement;
  const voiceSelect = h("select", { class: "field" }) as HTMLSelectElement;
  const qualitySelect = h("select", { class: "field" }) as HTMLSelectElement;
  const languageSelect = h("select", { class: "field" }) as HTMLSelectElement;

  qualitySelect.append(...["low", "medium", "high"].map((level) => h("option", { value: level }, QUALITY_LABEL[level])));
  qualitySelect.value = status.avatar.quality;
  languageSelect.append(
    ...["fr", "en", "es", "de", "it"].map((lang) => h("option", { value: lang }, lang)),
  );
  languageSelect.value = status.stt.language;

  for (const voice of status.tts.voices) {
    voiceSelect.append(h("option", { value: voice.id }, `${voice.label} — ${voice.description}`));
  }
  voiceSelect.value = status.tts.voice;

  modelSelect.append(
    h("option", { value: status.llm.model }, `${status.llm.model} (modèle actuel)`),
  );
  void api.listModels().catch(() => []).then((models) => {
    for (const model of models.slice(0, 80)) {
      if (model.model !== status.llm.model) {
        modelSelect.append(
          h(
            "option",
            { value: model.model },
            `${model.name}${model.free ? " — gratuit" : ""} · ${model.model}`,
          ),
        );
      }
    }
  });

  const steps = [
    {
      title: "Bonjour.",
      body: [
        h("p", {}, "Je suis Jimmy. Je vais vivre sur ton bureau et t'aider surtout à la voix."),
        h("p", { class: "note" }, "Ça prend quatre minutes. Chaque étape peut être corrigée plus tard dans les paramètres."),
      ],
    },
    {
      title: "Comment tu t'appelles ?",
      body: [h("p", {}, "C'est le prénom que j'utiliserai pour m'adresser à toi."), h("div", { class: "row" }, nameInput)],
    },
    {
      title: "Quel modèle pour réfléchir ?",
      body: [
        h("p", {}, "Le modèle qui me fait écrire le code, lire les fichiers et raisonner."),
        h("p", { class: "note" }, status.llm.has_key ? "Clé OpenCode Go détectée." : "Aucune clé détectée : OPENCODE_API_KEY dans le .env, ou saisie dans Paramètres → LLM."),
        h("div", { class: "row" }, modelSelect),
      ],
    },
    {
      title: "Quelle voix ?",
      body: [
        h("p", {}, "Ma voix par défaut, en français."),
        h("div", { class: "row" }, voiceSelect),
        h(
          "button",
          {
            class: "ghost",
            onclick: async () => {
              const voice = (voiceSelect as HTMLSelectElement).value;
              const label = status.tts.voices.find((v) => v.id === voice)?.label ?? "";
              await guard(() => api.ttsPreview(`Bonjour ${nameInput.value}, voici ma voix : ${label}.`), "essai vocal");
            },
          },
          "Écouter un essai",
        ),
      ],
    },
    {
      title: "Qualité de l'avatar",
      body: [
        h("p", {}, "« Basse » pour un portable modeste, « Haute » si tu as une machine récente."),
        h("div", { class: "row" }, qualitySelect),
        h(
          "button",
          {
            class: "ghost",
            onclick: async () => {
              const ok = await attempt(
                () => api.avatarQuality((qualitySelect as HTMLSelectElement).value),
                "qualité",
              );
              if (ok) toast("Qualité appliquée à l'avatar");
            },
          },
          "Appliquer et voir",
        ),
      ],
    },
    {
      title: "Langue de reconnaissance vocale",
      body: [h("p", {}, "La langue attendue quand tu me parles."), h("div", { class: "row" }, languageSelect)],
    },
    {
      title: "C'est prêt.",
      body: [
        h("p", {}, "En terminant, j'active l'écoute : j'ouvrirai le micro et resterai en veille."),
        h("p", {}, `Clique ensuite sur mon avatar pour revenir ici, ou dis simplement « ${capitalize(status.stt.wake_word)} ».`),
        h(
          "p",
          { class: "note" },
          "Première chose à essayer : « Jimmy, analyse ce dossier et explique-moi ce que tu trouves. »",
        ),
        h(
          "p",
          { class: "note" },
          "Pour couper l'écoute : page Voix → « Couper l'écoute ». Rien n'est envoyé sur le réseau.",
        ),
      ],
    },
  ];

  function render() {
    const current = steps[step];
    const footer = h(
      "div",
      { class: "onboarding-footer" },
      h("span", { class: "note" }, `Étape ${step + 1} sur ${steps.length}`),
      h(
        "div",
        { class: "row" },
        step > 0 ? h("button", { class: "ghost", onclick: () => { step -= 1; render(); } }, "Retour") : null,
        h(
          "button",
          {
            class: "primary",
            onclick: async () => {
              if (step < steps.length - 1) {
                step += 1;
                render();
                return;
              }
              // L'écran ne se ferme que si l'enregistrement a réussi.
              const done = await guard(
                () =>
                  api.completeOnboarding({
                    userName: nameInput.value.trim(),
                    model: (modelSelect as HTMLSelectElement).value,
                    voice: (voiceSelect as HTMLSelectElement).value,
                    quality: (qualitySelect as HTMLSelectElement).value,
                    language: (languageSelect as HTMLSelectElement).value,
                  }),
                "finalisation",
              );
              if (!done) return;
              ctx.status = done;
              onDone();
            },
          },
          step < steps.length - 1 ? "Suivant" : "Terminer",
        ),
      ),
    );
    mount(
      container,
      h(
        "div",
        { class: "onboarding-card" },
        h("h2", {}, current.title),
        ...current.body,
        footer,
      ),
    );
  }

  render();
  return container;
}