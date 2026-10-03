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
import { card } from "./settings";

export function voiceView(ctx: AppContext): HTMLElement {
  const recorder = new Recorder();
  let meterTimer = 0;

  // ── Écoute permanente : état lu côté Rust, jamais deviné ──────────────────
  const listenState = h("span", { class: "listen-state off" }, "…");
  const listenButton = h("button", { class: "primary", onclick: () => void toggleListening() }, "…");
  const servers = h("div", { class: "note" }, "");
  const heard = h("div", { class: "heard" });
  // Vumètre de l'écoute permanente : la preuve que le micro capte.
  const liveMeter = h("div", { class: "meter active", title: "Niveau du micro" }, h("div", { class: "meter-fill" }));
  const livePoll = window.setInterval(async () => {
    if (!ctx.voice?.running) return;
    const status = await api.voiceStatus().catch(() => null);
    if (!status) return;
    // Échelle perceptive : la parole à distance tourne autour de 0,005-0,02.
    const level = Math.min(1, Math.sqrt(status.level / 0.05));
    (liveMeter.firstElementChild as HTMLElement).style.width = `${level * 100}%`;
  }, 300);
  ctx.onCleanup(() => {
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
    listenButton.className = running ? "ghost" : "primary";
    listenButton.removeAttribute("disabled");
    liveMeter.style.display = running ? "" : "none";
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

  const voiceLabel =
    ctx.status.tts.voices.find((v) => v.id === ctx.status.tts.voice)?.label ?? ctx.status.tts.voice;

  return h(
    "section",
    { class: "view" },
    card(
      "Écoute permanente",
      h(
        "p",
        { class: "note" },
        "Jimmy garde le micro ouvert et attend « ",
        h("strong", {}, capitalize(ctx.status.stt.wake_word)),
        " ». La reconnaissance tourne en local : rien n'est envoyé sur le réseau. Si tu la laisses active, l'écoute reprend toute seule au prochain lancement.",
      ),
      h("div", { class: "row" }, listenButton, listenState),
      liveMeter,
      servers,
      h("h4", {}, "Ce que Jimmy entend"),
      heard,
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
      "Synthèse vocale",
      h("p", { class: "note" }, "Voix : ", h("code", {}, voiceLabel), " · modèle : ", h("code", {}, ctx.status.tts.model)),
      h("div", { class: "row" }, ttsInput, ttsButton),
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
        h("p", { class: "note" }, status.llm.has_key ? "Clé OpenCode Go détectée." : "Aucune clé détectée : renseigne OPENCODE_API_KEY dans le fichier .env."),
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