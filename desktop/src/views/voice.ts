/**
 * Vue Voix : microphone, reconnaissance, synthèse, mot d'activation.
 *
 * C'est la page qui rend vérifiable la chaîne vocale sans passer par le wake
 * word : on enregistre, on transcrit, on relit. Elle montre aussi ce que
 * comprend le détecteur de « Jimmy », pour diagnosing sans matériel particulier.
 */
import { api } from "../api";
import { Recorder } from "../audio";
import { attempt, guard, h, mount, toast } from "../ui";
import type { AppContext } from "../context";

export function voiceView(ctx: AppContext): HTMLElement {
  const recorder = new Recorder();
  let meterTimer = 0;
  let listening = false;

  const listenState = h("span", { class: "listen-state off" }, "arrêtée");
  const listenButton = h(
    "button",
    { class: "primary", onclick: () => void toggleListening() },
    "Activer l'écoute",
  );

  async function toggleListening() {
    if (listening) {
      await attempt(() => api.voiceStop(), "arrêt de l'écoute");
      listening = false;
      listenButton.textContent = "Activer l'écoute";
      listenButton.className = "primary";
      listenState.textContent = "arrêtée";
      listenState.className = "listen-state off";
      return;
    }
    listenButton.setAttribute("disabled", "");
    listenButton.textContent = "Démarrage…";
    // `attempt` et non `guard` : la commande ne retourne rien, donc `undefined`
    // ne dit rien du résultat.
    const ok = await attempt(() => api.voiceStart(), "démarrage de l'écoute");
    listenButton.removeAttribute("disabled");
    if (!ok) {
      listenButton.textContent = "Activer l'écoute";
      return;
    }
    listening = true;
    listenButton.textContent = "Désactiver l'écoute";
    listenButton.className = "ghost";
    listenState.textContent = "active";
    listenState.className = "listen-state on";
    void ctx.refreshStatus();
  }

  const meter = h("div", { class: "meter" }, h("div", { class: "meter-fill" }));
  const transcript = h("div", { class: "transcript" }, "Aucune transcription.");
  const recordButton = h(
    "button",
    { class: "primary", onclick: () => void toggleRecording() },
    "Enregistrer",
  );
  const ttsInput = h("input", {
    class: "field",
    type: "text",
    value: "Bonjour, je suis Jimmy.",
  }) as HTMLInputElement;

  async function toggleRecording() {
    if (!recorder.active) {
      const started = await attempt(() => recorder.start(), "micro");
      if (!started) return;
      recordButton.textContent = "Arrêter et transcrire";
      meter.classList.add("active");
      meterTimer = window.setInterval(() => {
        (meter.firstElementChild as HTMLElement).style.width = `${recorder.getLevel() * 100}%`;
      }, 60);
      return;
    }

    window.clearInterval(meterTimer);
    meter.classList.remove("active");
    recordButton.textContent = "Transcription…";
    const result = await recorder.stop();
    recordButton.textContent = "Enregistrer";

    if (result.wav.length === 0) {
      transcript.textContent = "Rien n'a été capturé.";
      return;
    }
    await ctx.refreshStatus();
    if (!ctx.status.stt.enabled) {
      toast("La reconnaissance vocale est désactivée dans les paramètres.", "error");
      transcript.textContent = "Reconnaissance vocale désactivée.";
      return;
    }
    transcript.textContent = "Transcription en cours…";
    const text = await guard(() => api.sttTranscribe(result.wav), "transcription");
    transcript.textContent = text ?? "Transcription impossible.";
    if (text) void api.avatarSay(text);
  }

  const wakeInput = h("input", {
    class: "field",
    type: "text",
    value: "J'y mise, analyse ce dossier",
  }) as HTMLInputElement;
  const wakeResult = h("div", { class: "note" }, "—");

  async function testWake() {
    const result = await guard(
      () => api.wakeWordStrip(wakeInput.value, ctx.status.stt.wake_word),
      "test du mot d'activation",
    );
    if (!result) return;
    wakeResult.textContent = result.matched
      ? `Détecté — commande retenue : « ${result.command} »`
      : "Non détecté : cette phrase ne commence pas par le mot d'activation.";
  }

  void api.voiceDevices().then((devices) => {
    deviceList.textContent =
      devices.length > 0 ? devices.join(" · ") : "Aucun microphone détecté par Windows.";
  });

  const deviceList = h("div", { class: "note" }, "Recherche des microphones…");

  return h(
    "section",
    { class: "view" },
    h("header", { class: "view-header" }, h("h2", {}, "Voix")),
    h(
      "section",
      { class: "card" },
      h("h3", {}, "Écoute permanente"),
      h(
        "p",
        { class: "note" },
        "Quand l'écoute est active, Jimmy ouvre le microphone, reste en veille et attend « ",
        h("code", {}, ctx.status.stt.wake_word),
        " ». Rien n'est envoyé sur le réseau : la reconnaissance tourne en local.",
      ),
      h("div", { class: "row" }, listenButton, listenState),
    ),
    h(
      "section",
      { class: "card" },
      h("h3", {}, "Reconnaissance vocale"),
      h(
        "p",
        { class: "note" },
        "Modèle : ",
        h("code", {}, ctx.status.stt.model),
        " · langue : ",
        h("code", {}, ctx.status.stt.language),
      ),
      meter,
      h(
        "div",
        { class: "row" },
        recordButton,
        h(
          "button",
          {
            class: "ghost",
            onclick: () => {
              transcript.textContent = "Aucune transcription.";
            },
          },
          "Effacer",
        ),
      ),
      transcript,
      deviceList,
    ),
    h(
      "section",
      { class: "card" },
      h("h3", {}, "Synthèse vocale"),
      h(
        "p",
        { class: "note" },
        "Voix : ",
        h(
          "code",
          {},
          ctx.status.tts.voices.find((v) => v.id === ctx.status.tts.voice)?.label ??
            ctx.status.tts.voice,
        ),
        " · modèle : ",
        h("code", {}, ctx.status.tts.model),
      ),
      h(
        "div",
        { class: "row" },
        ttsInput,
        h(
          "button",
          {
            class: "primary",
            onclick: async () => {
              const text = ttsInput.value.trim();
              if (!text) return;
              const result = await guard(() => api.ttsPreview(text), "synthèse vocale");
              if (result) toast(`Audio généré : ${(result.bytes / 1024).toFixed(0)} Ko`);
            },
          },
          "Lire",
        ),
      ),
    ),
    h(
      "section",
      { class: "card" },
      h("h3", {}, "Mot d'activation"),
      h(
        "p",
        { class: "note" },
        "Le wake word est reconnu localement. Comme Whisper approxime les mots courts, ",
        "le détecteur accepte ses variantes courantes et une distance d'édition bornée.",
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

  const nameInput = h("input", { class: "field", type: "text", value: "Jimmy" }) as HTMLInputElement;
  const modelSelect = h("select", { class: "field" }) as HTMLSelectElement;
  const voiceSelect = h("select", { class: "field" }) as HTMLSelectElement;
  const qualitySelect = h("select", { class: "field" }) as HTMLSelectElement;
  const languageSelect = h("select", { class: "field" }) as HTMLSelectElement;

  qualitySelect.append(...["low", "medium", "high"].map((level) => h("option", { value: level }, level)));
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
  void api.listModels().then((models) => {
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
      title: "Comment tu veux m'appeler ?",
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
        h("p", {}, "« low » pour un portable modeste, « high » si tu as une machine récente."),
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
        h("p", {}, "Clique ensuite sur mon avatar pour revenir ici, ou dis simplement « Jimmy »."),
        h(
          "p",
          { class: "note" },
          "Première chose à essayer : « Jimmy, analyse ce dossier et explique-moi ce que tu trouves. »",
        ),
        h(
          "p",
          { class: "note" },
          "Pour couper l'écoute : page Voix → « Désactiver l'écoute ». Rien n'est envoyé sur le réseau.",
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
              await guard(
                () =>
                  api.completeOnboarding({
                    userName: nameInput.value,
                    model: (modelSelect as HTMLSelectElement).value,
                    voice: (voiceSelect as HTMLSelectElement).value,
                    quality: (qualitySelect as HTMLSelectElement).value,
                    language: (languageSelect as HTMLSelectElement).value,
                  }),
                "finalisation",
              );
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