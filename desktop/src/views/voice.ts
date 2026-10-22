/**
 * Vue Voix : microphone, reconnaissance, synthèse, mot d'activation.
 *
 * C'est la page qui rend vérifiable la chaîne vocale sans passer par le wake
 * word : on enregistre, on transcrit, on relit. Elle montre aussi ce que
 * comprend le détecteur de « Jimy », pour diagnosing sans matériel particulier.
 */
import { api } from "../api";
import { Recorder } from "../audio";
import { attempt, capitalize, guard, h, mount, qualityLabel, toast } from "../ui";
import type { AppContext } from "../context";
import { lang, t } from "../i18n";
import { card, field, toggle, FOLLOW_UP_CHOICES, HESITATION_CHOICES, SPOKEN_LENGTH_CHOICES, LANGUAGES } from "./settings";
import { modelsPanel } from "./models";
import { voicesPanel } from "./voices";

/** Modèle TTS par défaut, aligné sur `TtsSettings::default()` côté Rust. */
const DEFAULT_TTS_MODEL = "fish-audio/s2.1-pro-free:free";

export function voiceView(ctx: AppContext): HTMLElement {
  const recorder = new Recorder();
  let meterTimer = 0;

  // ── Écoute permanente : état lu côté Rust, jamais deviné ──────────────────
  const listenState = h("span", { class: "listen-state off" }, "…");
  const listenButton = h("button", { class: "primary", onclick: () => void toggleListening() }, "…");
  const servers = h("div", { class: "note" }, "");
  const heard = h("div", { class: "heard" });

  // Bandeau de phase : à chaque instant, ce que Jimy attend de toi.
  const phaseText = h("span", { class: "phase-text" }, "…");
  const banner = h("div", { class: "phase-banner idle", role: "status" }, h("span", { class: "phase-dot" }), phaseText);
  let phaseTimer = 0;
  const wake = capitalize(ctx.status.stt.wake_word);

  function showPhase(phase: string, remaining = 0) {
    window.clearInterval(phaseTimer);
    banner.className = `phase-banner ${phase}`;
    const labels: Record<string, string> = {
      idle: t("voice.phase.idle", { wake }),
      capturing: t("voice.phase.capturing"),
      transcribing: t("voice.phase.transcribing"),
      thinking: t("voice.phase.thinking"),
      speaking: t("voice.phase.speaking"),
    };
    if (phase === "your_turn" && remaining > 0) {
      const end = Date.now() + remaining;
      const tick = () => {
        const s = Math.max(0, Math.ceil((end - Date.now()) / 1000));
        phaseText.textContent = t("voice.phase.yourTurn", { s });
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
  const liveMeter = h("div", { class: "meter active", title: t("voice.micLevel") }, h("div", { class: "meter-fill" }));
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
    listenState.textContent = running ? t("voice.state.on") : t("voice.state.off");
    listenState.className = `listen-state ${running ? "on" : "off"}`;
    listenButton.textContent = running ? t("voice.button.stop") : t("voice.button.start");
    listenButton.className = `${running ? "ghost" : "primary"} voice-listen`;
    listenButton.removeAttribute("disabled");
    liveMeter.style.display = running ? "" : "none";
    wave.classList.toggle("off", !running);
    if (!running) drawWave(0);
    banner.style.display = running ? "" : "none";
    if (running && banner.classList.contains("idle")) showPhase("idle");
    if (!voice) {
      servers.textContent = t("voice.statusUnavailable");
      return;
    }
    const ready = (ok: boolean) => (ok ? t("voice.server.ready") : running ? t("voice.server.unavailable") : t("voice.server.idle"));
    mount(
      servers,
      t("voice.server.wake"),
      h("code", {}, voice.wakeModel),
      t("voice.server.command", { state: ready(voice.wakeReady) }),
      h("code", {}, voice.commandModel || voice.wakeModel),
      ` (${ready(voice.commandReady || (!voice.commandModel && voice.wakeReady))})`,
      voice.deviceRate ? t("voice.server.micRate", { rate: voice.deviceRate }) : "",
    );
  }

  async function toggleListening() {
    const running = Boolean(ctx.voice?.running);
    listenButton.setAttribute("disabled", "");
    listenButton.textContent = running ? t("voice.button.stopping") : t("voice.button.starting");
    const ok = running
      ? await attempt(() => api.voiceStop(), t("guard.listenStop"))
      : await attempt(() => api.voiceStart(), t("guard.listenStart"));
    await ctx.refreshStatus();
    renderListening();
    if (ok && !running) toast(t("toast.listening", { word: capitalize(ctx.status.stt.wake_word) }));
  }

  // ── Interrupteur principal de la voix (Lot 2) ──────────────────────────
  // Coupe ou réactive toute la voix à la volée : écoute micro/VAD + synthèse
  // TTS. Persisté côté Rust (`voice_set_enabled`) et respecté au prochain
  // démarrage. La boucle de l'agent n'est jamais touchée : sans voix, les
  // réponses continuent à l'écran et dans la bulle.
  const voiceMasterToggle = h("input", { type: "checkbox" }) as HTMLInputElement;
  const listenToggle = h("input", { type: "checkbox" }) as HTMLInputElement;
  const speechToggle = h("input", { type: "checkbox" }) as HTMLInputElement;
  const voiceStateNote = h("p", { class: "note voice-enabled-note" }, t("voice.loading"));

  async function loadVoiceEnabled() {
    const loaded = await guard(() => api.getSettings(), t("guard.voiceEnable"));
    if (!loaded) return;
    voiceMasterToggle.checked = loaded.voice.enabled;
    listenToggle.checked = loaded.stt.enabled;
    speechToggle.checked = loaded.tts.enabled;
    renderVoiceEnabled();
  }

  function renderVoiceEnabled() {
    const master = voiceMasterToggle.checked;
    const listening = listenToggle.checked && master;
    const speaking = speechToggle.checked && master;
    voiceStateNote.textContent = !master
      ? t("voice.enabled.allOff")
      : !listening && !speaking
        ? t("voice.enabled.bothSubOff")
        : !listening
          ? t("voice.enabled.micOff")
          : !speaking
            ? t("voice.enabled.ttsOff")
            : t("voice.enabled.allOn");
    listenToggle.disabled = !master;
    speechToggle.disabled = !master;
    // Sans micro, le bouton d'écoute n'a aucun sens.
    listenButton.toggleAttribute("disabled", !listening && !ctx.voice?.running);
  }

  async function setVoiceEnabled(scope: "voice" | "listen" | "speech", box: HTMLInputElement) {
    const wanted = box.checked;
    box.disabled = true;
    const updated = await guard(() => api.voiceSetEnabled(scope, wanted), t("guard.voiceEnable"));
    if (updated) {
      voiceMasterToggle.checked = updated.voice;
      listenToggle.checked = updated.listen;
      speechToggle.checked = updated.speech;
      await ctx.refreshStatus();
      renderListening();
      toast(
        scope === "voice"
          ? wanted
            ? t("voice.toast.voiceOn")
            : t("voice.toast.voiceOff")
          : scope === "listen"
            ? wanted
              ? t("voice.toast.micOn")
              : t("voice.toast.micOff")
            : wanted
              ? t("voice.toast.ttsOn")
              : t("voice.toast.ttsOff"),
      );
    } else {
      box.checked = !wanted;
    }
    box.disabled = false;
    renderVoiceEnabled();
  }

  voiceMasterToggle.addEventListener("change", () => void setVoiceEnabled("voice", voiceMasterToggle));
  listenToggle.addEventListener("change", () => void setVoiceEnabled("listen", listenToggle));
  speechToggle.addEventListener("change", () => void setVoiceEnabled("speech", speechToggle));
  void loadVoiceEnabled();

  function emptyHeard() {
    mount(heard, h("p", { class: "hint" }, t("voice.heard.empty")));
  }

  // Fil « ce que Jimy entend » : le retour qui manquait pour savoir si le
  // micro capte, et si le mot d'éveil est reconnu.
  ctx.onEvent((event) => {
    if (event.type === "listen") showPhase(event.phase ?? "idle", event.remaining ?? 0);
    // Après « Oui ? » ou une réponse, Jimy attend la suite : on le dit clairement.
    if (event.type === "state" && event.state === "listening" && (event.detail === "Oui ?" || event.detail === "À toi")) {
      heard.querySelector(".hint")?.remove();
      heard.prepend(
        h(
          "div",
          { class: "heard-line matched" },
          h("span", { class: "heard-tag" }, t("voice.heard.yourTurnTag")),
          h("span", {}, event.detail === "À toi" ? t("voice.heard.followUp") : t("voice.heard.speakNow")),
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
        h("span", { class: "heard-tag" }, event.matched ? t("voice.heard.wakeTag") : t("voice.heard.heardTag")),
        h("span", {}, event.text ?? ""),
      ),
    );
    while (heard.childElementCount > 8) heard.lastElementChild?.remove();
  });

  // ── Test de reconnaissance (micro de la fenêtre) ──────────────────────────
  const meter = h("div", { class: "meter" }, h("div", { class: "meter-fill" }));
  const transcript = h("div", { class: "transcript" }, t("voice.record.none"));
  const recordButton = h("button", { class: "primary", onclick: () => void toggleRecording() }, t("voice.record.start"));

  async function toggleRecording() {
    if (!recorder.active) {
      const started = await attempt(() => recorder.start(), t("guard.mic"));
      if (!started) return;
      recordButton.textContent = t("voice.record.stop");
      meter.classList.add("active");
      meterTimer = window.setInterval(() => {
        (meter.firstElementChild as HTMLElement).style.width = `${Math.min(1, recorder.getLevel() * 4) * 100}%`;
      }, 60);
      return;
    }
    window.clearInterval(meterTimer);
    meter.classList.remove("active");
    recordButton.setAttribute("disabled", "");
    recordButton.textContent = t("voice.record.transcribing");
    const result = await recorder.stop();
    if (result.wav.length === 0) {
      transcript.textContent = t("voice.record.nothingCaptured");
    } else {
      transcript.textContent = t("voice.record.transcribingFirst");
      // Les serveurs démarrent à la demande côté Rust : pas besoin d'avoir
      // activé l'écoute permanente.
      const text = await guard(() => api.sttTranscribe(result.wav), t("guard.transcription"));
      transcript.textContent = text === undefined ? t("voice.record.impossible") : text || t("voice.record.silence");
    }
    recordButton.removeAttribute("disabled");
    recordButton.textContent = t("voice.record.start");
  }

  // ── Synthèse vocale ───────────────────────────────────────────────────────
  const ttsInput = h("input", { class: "field", type: "text", value: t("voice.tts.defaultSample") }) as HTMLInputElement;
  const ttsButton = h("button", { class: "primary", onclick: () => void speak() }, t("voice.tts.read"));

  async function speak() {
    const text = ttsInput.value.trim();
    if (!text) return;
    ttsButton.setAttribute("disabled", "");
    ttsButton.textContent = t("voice.tts.synthesizing");
    await guard(() => api.ttsPreview(text), t("guard.tts"));
    ttsButton.removeAttribute("disabled");
    ttsButton.textContent = t("voice.tts.read");
  }

  // ── Mot d'activation ──────────────────────────────────────────────────────
  const wakeInput = h("input", { class: "field", type: "text", value: t("voice.wake.defaultSample") }) as HTMLInputElement;
  const wakeResult = h("div", { class: "note" }, "—");

  async function testWake() {
    const result = await guard(
      () => api.wakeWordStrip(wakeInput.value, ctx.status.stt.wake_word),
      t("guard.wakeTest"),
    );
    if (!result) return;
    wakeResult.textContent = result.matched
      ? t("voice.wake.matched", { command: result.command || t("voice.wake.noCommand") })
      : t("voice.wake.notMatched");
  }

  const deviceList = h("div", { class: "note" }, t("voice.devices.searching"));
  void api
    .voiceDevices()
    .then((devices) => {
      deviceList.textContent =
        devices.length > 0 ? t("voice.devices.list", { list: devices.join(" · ") }) : t("voice.devices.none");
    })
    .catch(() => {
      deviceList.textContent = t("voice.devices.unavailable");
    });

  emptyHeard();
  renderListening();
  void ctx.refreshStatus().then(renderListening);

  // Bibliothèque de voix : ici, dans l'onglet Voix, là où on la cherche (elle
  // était enterrée dans les Paramètres, sous la liste des modèles).
  const voices = voicesPanel({ onApplied: () => void ctx.refreshStatus() });

  // ── Réglages voix et écoute (rapatriés des Paramètres) ───────────────────
  // Tout ce qui touche à la voix de Jimy vit dans cet onglet : modèle vocal
  // LLM, synthèse, STT, mot d'activation. Enregistrement dédié : on repart de
  // la copie fraîche de Rust pour ne rien écraser des autres onglets.
  const voiceModelInput = h("input", { class: "field", type: "text" }) as HTMLInputElement;
  const sttSelect = h("select", { class: "field" }) as HTMLSelectElement;
  const commandSelect = h("select", { class: "field" }) as HTMLSelectElement;
  const languageSelect = h("select", { class: "field" }) as HTMLSelectElement;
  const wakeWordInput = h("input", { class: "field", type: "text" }) as HTMLInputElement;
  const cuesEnabled = h("input", { type: "checkbox" }) as HTMLInputElement;
  const followSelect = h("select", { class: "field" }) as HTMLSelectElement;
  const pauseInput = h("select", { class: "field" }) as HTMLSelectElement;
  const debugAudio = h("input", { type: "checkbox" }) as HTMLInputElement;

  async function loadVoiceSettings() {
    const loaded = await guard(() => api.getSettings(), t("guard.voiceSettings"));
    if (!loaded) return;
    voiceModelInput.value = loaded.llm.voice_model ?? "";
    mount(sttSelect);
    for (const model of ctx.status.stt.models) {
      sttSelect.append(h("option", { value: model.id }, `${model.label} — ${model.note}`));
    }
    sttSelect.value = loaded.stt.model;
    mount(commandSelect);
    commandSelect.append(h("option", { value: "" }, t("voice.commandSameAsWake")));
    for (const model of ctx.status.stt.models) {
      commandSelect.append(h("option", { value: model.id }, `${model.label} — ${model.note}`));
    }
    commandSelect.value = loaded.stt.command_model;
    mount(languageSelect);
    for (const [code, labelKey] of LANGUAGES) {
      languageSelect.append(h("option", { value: code }, t(labelKey)));
    }
    languageSelect.value = loaded.stt.language;
    wakeWordInput.value = loaded.stt.wake_word;
    cuesEnabled.checked = loaded.tts.cues;
    mount(followSelect);
    for (const [seconds, labelKey] of FOLLOW_UP_CHOICES) {
      followSelect.append(h("option", { value: String(seconds) }, t(labelKey)));
    }
    // Une valeur personnalisée (fichier de config édité) reste sélectionnable.
    const current = Math.round(loaded.voice.follow_up_ms / 1000);
    if (![...FOLLOW_UP_CHOICES].some(([s]) => s === current)) {
      followSelect.append(h("option", { value: String(current) }, `${current} s`));
    }
    followSelect.value = String(current);
    mount(pauseInput);
    for (const [ms, labelKey] of HESITATION_CHOICES) {
      pauseInput.append(h("option", { value: String(ms) }, t(labelKey)));
    }
    // Une valeur personnalisée (fichier de config édité) reste sélectionnable.
    const pauseCurrent = loaded.voice.end_of_speech_ms;
    if (![...HESITATION_CHOICES].some(([ms]) => ms === pauseCurrent)) {
      pauseInput.append(h("option", { value: String(pauseCurrent) }, `${pauseCurrent} ms`));
    }
    pauseInput.value = String(pauseCurrent);
    debugAudio.checked = loaded.voice.debug_audio;
  }

  async function persistVoice() {
    const fresh = await guard(() => api.getSettings(), t("guard.voiceSettings"));
    if (!fresh) return;
    fresh.llm.voice_model = voiceModelInput.value.trim();
    fresh.stt.model = sttSelect.value;
    fresh.stt.command_model = commandSelect.value;
    fresh.stt.language = languageSelect.value;
    fresh.stt.wake_word = wakeWordInput.value.trim() || "jimy";
    fresh.tts.cues = cuesEnabled.checked;
    fresh.voice.follow_up_ms = Number(followSelect.value) * 1000;
    fresh.voice.end_of_speech_ms = Math.min(2000, Math.max(400, Number(pauseInput.value) || 1200));
    fresh.voice.debug_audio = debugAudio.checked;
    const before = ctx.status.stt;
    if (!(await attempt(() => api.saveSettings(fresh), t("guard.save")))) return;
    await ctx.refreshStatus();
    const sttChanged =
      before.model !== fresh.stt.model ||
      before.command_model !== fresh.stt.command_model ||
      before.language !== fresh.stt.language;
    toast(
      sttChanged && ctx.voice?.running
        ? t("voice.savedRestartNeeded")
        : t("voice.saved"),
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

  // ── Synthèse vocale : clé OpenRouter et modèle TTS ───────────────────────
  // La clé est celle du fournisseur OpenRouter (partagée avec Paramètres →
  // LLM) : Rust la lit via `Settings::tts_key`, saisie d'abord, `.env` ensuite.
  const openrouter = ctx.status.llm.providers.find((p) => p.id === "openrouter");
  const ttsKeyState = h(
    "p",
    { class: "note tts-key-state" },
    openrouter?.key_from === "interface"
      ? t("voice.openrouter.ui", { hint: openrouter.key_hint ?? "" })
      : openrouter?.key_from === "environnement"
        ? t("voice.openrouter.env")
        : t("voice.openrouter.missing"),
  );
  const ttsKeyInput = h("input", {
    class: "field tts-key",
    type: "password",
    autocomplete: "off",
    placeholder: openrouter?.key_from === "interface" ? t("voice.openrouter.unchanged") : "sk-or-…",
  }) as HTMLInputElement;
  const ttsModelInput = h("input", { class: "field tts-model", type: "text", value: ctx.status.tts.model }) as HTMLInputElement;
  const spokenLengthSelect = h("select", { class: "field tts-length" }) as HTMLSelectElement;

  async function persistTts() {
    // Clé d'abord : une erreur garde la saisie à l'écran.
    const key = ttsKeyInput.value.trim();
    if (key && !(await attempt(() => api.llmSetProviderKey("openrouter", key), t("guard.openrouterKey")))) return;
    // Copie fraîche de Rust : la clé vient d'y être écrite, ne pas l'écraser.
    const fresh = await guard(() => api.getSettings(), t("guard.voiceSettings"));
    if (!fresh) return;
    fresh.tts.model = ttsModelInput.value.trim() || DEFAULT_TTS_MODEL;
    fresh.tts.max_spoken_chars = Math.min(20000, Math.max(100, Number(spokenLengthSelect.value) || 900));
    if (!(await attempt(() => api.saveSettings(fresh), t("guard.save")))) return;
    ttsKeyInput.value = "";
    ttsModelInput.value = fresh.tts.model;
    syncSpokenLength(fresh.tts.max_spoken_chars);
    await ctx.refreshStatus();
    toast(t("voice.ttsSaved"));
  }

  function syncSpokenLength(value: number) {
    mount(spokenLengthSelect);
    for (const [chars, labelKey] of SPOKEN_LENGTH_CHOICES) {
      spokenLengthSelect.append(h("option", { value: String(chars) }, t(labelKey)));
    }
    // Une valeur personnalisée (fichier de config édité) reste sélectionnable.
    if (![...SPOKEN_LENGTH_CHOICES].some(([chars]) => chars === value)) {
      spokenLengthSelect.append(h("option", { value: String(value) }, t("voice.charsCustom", { n: value })));
    }
    spokenLengthSelect.value = String(value);
  }

  // Longueur lue : valeur réglée côté Rust, 900 par défaut (config ancienne).
  void guard(() => api.getSettings(), t("guard.voiceSettings")).then((loaded) => {
    if (loaded) syncSpokenLength(loaded.tts.max_spoken_chars ?? 900);
  });

  // Installation neuve : whisper.cpp se télécharge au premier lancement.
  // Tant qu'il manque, l'écoute ne peut pas démarrer : on dit où on en est.
  const comp = ctx.status.stt.components;
  const componentsNote = comp.ready
    ? null
    : h(
        "div",
        { class: "warn-note" },
        comp.active
          ? t("voice.comp.downloading", {
              current: comp.current,
              pct: comp.total_bytes > 0 ? Math.floor((comp.done_bytes * 100) / comp.total_bytes) : 0,
              mb: Math.round(comp.done_bytes / 1048576),
            })
          : t("voice.comp.missing", { error: comp.error ? ` (${comp.error})` : "" }),
      );

  return h(
    "section",
    { class: "view" },
    componentsNote,
    card(
      t("voice.enableCard.title"),
      h(
        "p",
        { class: "note" },
        t("voice.enableCard.note"),
      ),
      toggle(t("voice.enableCard.master"), voiceMasterToggle),
      toggle(t("voice.enableCard.mic"), listenToggle),
      toggle(t("voice.enableCard.tts"), speechToggle),
      voiceStateNote,
    ),
    h(
      "div",
      { class: "card voice-hero" },
      h(
        "div",
        { class: "voice-hero-head" },
        h("h3", { class: "kicker" }, t("voice.hero.kicker")),
        h("h2", { class: "voice-hero-word" }, "« ", h("em", {}, capitalize(ctx.status.stt.wake_word)), " »"),
        h(
          "p",
          { class: "note" },
          t("voice.hero.noteBefore"),
          h("strong", {}, capitalize(ctx.status.stt.wake_word)),
          t("voice.hero.noteAfter"),
        ),
      ),
      wave,
      h("div", { class: "voice-hero-actions" }, listenButton, listenState),
      banner,
      liveMeter,
      servers,
      h("h4", {}, t("voice.hero.heardTitle")),
      heard,
    ),
    card(
      t("voice.modelCard.title"),
      h(
        "p",
        { class: "note" },
        t("voice.modelCard.note"),
      ),
      field(t("voice.modelCard.field"), voiceModelInput, t("voice.modelCard.placeholder")),
      h(
        "div",
        { class: "row" },
        h("button", { class: "ghost", onclick: () => void loadVoiceSettings() }, t("settings.cancel")),
        h("button", { class: "primary", onclick: () => void persistVoice() }, t("settings.save")),
      ),
      voiceModels,
    ),
    card(
      t("voice.listenCard.title"),
      h(
        "p",
        { class: "note" },
        t("voice.listenCard.note"),
      ),
      field(t("voice.listenCard.wakeModel"), sttSelect),
      field(t("voice.listenCard.cmdModel"), commandSelect),
      field(t("voice.listenCard.language"), languageSelect),
      field(t("voice.listenCard.wakeWord"), wakeWordInput, "jimy"),
      // Skin premium actif : il répond à son nom, le réglage ci-dessus ne
      // vaut que pour les skins de base.
      ctx.status.stt.wake_word !== ctx.status.stt.wake_word_setting
        ? h(
            "p",
            { class: "note" },
            t("voice.listenCard.premiumWakeNote", { word: capitalize(ctx.status.stt.wake_word) }),
          )
        : null,
      field(t("voice.listenCard.followUp"), followSelect),
      field(t("voice.listenCard.pause"), pauseInput),
      h(
        "p",
        { class: "note" },
        t("voice.listenCard.pauseNote"),
      ),
      toggle(t("voice.listenCard.cues"), cuesEnabled),
      toggle(t("voice.listenCard.debugAudio"), debugAudio),
      h(
        "div",
        { class: "row" },
        h("button", { class: "ghost", onclick: () => void loadVoiceSettings() }, t("settings.cancel")),
        h("button", { class: "primary", onclick: () => void persistVoice() }, t("settings.save")),
      ),
    ),
    card(
      t("voice.voicesCard.title"),
      h(
        "p",
        { class: "note" },
        t("voice.voicesCard.note"),
      ),
      voices,
      h("h4", {}, t("voice.voicesCard.readTitle")),
      h("div", { class: "row" }, ttsInput, ttsButton),
    ),
    card(
      t("voice.ttsCard.title"),
      h(
        "p",
        { class: "note" },
        t("voice.ttsCard.note"),
      ),
      ttsKeyState,
      field(t("voice.ttsCard.key"), ttsKeyInput),
      field(t("voice.ttsCard.model"), ttsModelInput, DEFAULT_TTS_MODEL),
      field(t("voice.ttsCard.length"), spokenLengthSelect),
      h(
        "p",
        { class: "note" },
        t("voice.ttsCard.familyNote"),
      ),
      h(
        "div",
        { class: "row" },
        h("button", { class: "ghost", onclick: () => (ttsModelInput.value = DEFAULT_TTS_MODEL) }, t("voice.ttsCard.defaultModel")),
        h("button", { class: "primary", onclick: () => void persistTts() }, t("settings.save")),
      ),
    ),
    card(
      t("voice.testStt.title"),
      h("p", { class: "note" }, t("voice.testStt.note")),
      meter,
      h(
        "div",
        { class: "row" },
        recordButton,
        h("button", { class: "ghost", onclick: () => (transcript.textContent = t("voice.record.none")) }, t("voice.testStt.clear")),
      ),
      transcript,
      deviceList,
    ),
    card(
      t("voice.testWake.title"),
      h(
        "p",
        { class: "note" },
        t("voice.testWake.note"),
      ),
      h("div", { class: "row" }, wakeInput, h("button", { class: "ghost", onclick: () => void testWake() }, t("providers.test"))),
      wakeResult,
    ),
  );
}

/** Onboarding : la toute première expérience, conversationnelle et vérifiable. */
export function onboardingView(ctx: AppContext, onDone: () => void): HTMLElement {
  let step = 0;
  const container = h("section", { class: "view onboarding" });
  const status = ctx.status;

  const nameInput = h("input", { class: "field", type: "text", placeholder: t("onb.namePlaceholder") }) as HTMLInputElement;
  const modelSelect = h("select", { class: "field" }) as HTMLSelectElement;
  const voiceSelect = h("select", { class: "field" }) as HTMLSelectElement;
  const qualitySelect = h("select", { class: "field" }) as HTMLSelectElement;
  const languageSelect = h("select", { class: "field" }) as HTMLSelectElement;
  // Clé du modèle de langage : sans elle, rien ne répond (installation neuve).
  const keyProvider = h(
    "select",
    { class: "field" },
    h("option", { value: "opencode" }, "OpenCode Go"),
    h("option", { value: "openrouter" }, "OpenRouter"),
  ) as HTMLSelectElement;
  const keyInput = h("input", { class: "field", type: "password", placeholder: t("onb.keyPlaceholder"), autocomplete: "off" }) as HTMLInputElement;
  // Accès : une nouvelle installation part de « tout refusé ». Seule la
  // lecture est cochée d'office ; le reste est un choix explicite.
  const grant = (label: string, note: string, checked: boolean) => {
    const box = h("input", { type: "checkbox" }) as HTMLInputElement;
    box.checked = checked;
    return { box, row: h("label", { class: "grant-row" }, box, h("span", {}, h("strong", {}, label), ` — ${note}`)) };
  };
  const grants = {
    read: grant(t("onb.grant.read"), t("onb.grant.readNote"), true),
    write: grant(t("onb.grant.write"), t("onb.grant.writeNote"), false),
    execute: grant(t("onb.grant.execute"), t("onb.grant.executeNote"), false),
    network: grant(t("onb.grant.network"), t("onb.grant.networkNote"), false),
  };

  qualitySelect.append(...["low", "medium", "high"].map((level) => h("option", { value: level }, qualityLabel(level))));
  qualitySelect.value = status.avatar.quality;
  // Noms des langues dans leur propre langue : lisibles quelle que soit
  // celle de l'interface.
  const sttLanguages: [string, string][] = [["fr", "Français"], ["en", "English"], ["es", "Español"], ["de", "Deutsch"], ["it", "Italiano"]];
  languageSelect.append(...sttLanguages.map(([code, name]) => h("option", { value: code }, name)));
  // Installation neuve : la reconnaissance suit la langue de l'interface.
  languageSelect.value = status.stt.language === "fr" && lang() === "en" ? "en" : status.stt.language;

  // Langue de l'interface, choisie dès le premier écran (version anglaise,
  // déploiement en masse). Enregistrée tout de suite, puis l'accueil est
  // rechargé dans la langue choisie (on est encore à la première étape).
  const uiLanguageSelect = h("select", { class: "field", "aria-label": t("onb.uiLanguage") }) as HTMLSelectElement;
  for (const value of ["auto", "fr", "en"] as const) uiLanguageSelect.append(h("option", { value }, t(`lang.${value}`)));
  void api.getSettings().then((s) => (uiLanguageSelect.value = s.ui.language || "auto")).catch(() => undefined);
  uiLanguageSelect.addEventListener("change", async () => {
    const fresh = await guard(() => api.getSettings(), t("lang.title"));
    if (!fresh) return;
    fresh.ui.language = uiLanguageSelect.value;
    if (await attempt(() => api.saveSettings(fresh), t("lang.title"))) window.location.reload();
  });

  for (const voice of status.tts.voices) {
    voiceSelect.append(h("option", { value: voice.id }, `${voice.label} — ${voice.description}`));
  }
  voiceSelect.value = status.tts.voice;

  modelSelect.append(
    h("option", { value: status.llm.model }, t("onb.currentModel", { model: status.llm.model })),
  );
  void api.listModels().catch(() => []).then((models) => {
    for (const model of models.slice(0, 80)) {
      if (model.model !== status.llm.model) {
        modelSelect.append(
          h(
            "option",
            { value: model.model },
            `${model.name}${model.free ? t("onb.free") : ""} · ${model.model}`,
          ),
        );
      }
    }
  });

  const steps = [
    {
      title: t("onb.hello"),
      body: [
        h("p", {}, t("onb.intro")),
        h("p", { class: "note" }, t("onb.introNote")),
        h("label", { class: "field-row" }, h("span", {}, t("onb.uiLanguage")), uiLanguageSelect),
      ],
    },
    {
      title: t("onb.name"),
      body: [h("p", {}, t("onb.nameNote")), h("div", { class: "row" }, nameInput)],
    },
    {
      title: t("onb.model"),
      body: [
        h("p", {}, t("onb.modelNote")),
        status.llm.has_key
          ? h("p", { class: "note" }, t("onb.keyFound"))
          : h(
              "div",
              {},
              h(
                "p",
                { class: "note" },
                t("onb.keyNeeded"),
              ),
              h("div", { class: "row" }, keyProvider, keyInput),
            ),
        h("div", { class: "row" }, modelSelect),
      ],
    },
    {
      title: t("onb.voice"),
      body: [
        h("p", {}, t("onb.voiceNote")),
        h("div", { class: "row" }, voiceSelect),
        h(
          "button",
          {
            class: "ghost",
            onclick: async () => {
              const voice = (voiceSelect as HTMLSelectElement).value;
              const label = status.tts.voices.find((v) => v.id === voice)?.label ?? "";
              await guard(() => api.ttsPreview(t("onb.voicePreview", { name: nameInput.value, label })), t("guard.voicePreview"));
            },
          },
          t("onb.listen"),
        ),
      ],
    },
    {
      title: t("onb.quality"),
      body: [
        h("p", {}, t("onb.qualityNote")),
        h("div", { class: "row" }, qualitySelect),
        h(
          "button",
          {
            class: "ghost",
            onclick: async () => {
              const ok = await attempt(
                () => api.avatarQuality((qualitySelect as HTMLSelectElement).value),
                t("guard.quality"),
              );
              if (ok) toast(t("onb.qualityApplied"));
            },
          },
          t("onb.applyAndSee"),
        ),
      ],
    },
    {
      title: t("onb.sttLanguage"),
      body: [h("p", {}, t("onb.sttLanguageNote")), h("div", { class: "row" }, languageSelect)],
    },
    {
      title: t("onb.grants"),
      body: [
        h("p", {}, t("onb.grantsNote")),
        ...Object.values(grants).map((g) => g.row),
        h("p", { class: "note" }, t("onb.grantsLater")),
      ],
    },
    {
      title: t("onb.ready"),
      body: [
        h("p", {}, t("onb.readyListen")),
        h("p", {}, t("onb.readyComeBack", { word: capitalize(status.stt.wake_word) })),
        h(
          "p",
          { class: "note" },
          t("onb.readyTry"),
        ),
        h(
          "p",
          { class: "note" },
          t("onb.readyMute"),
        ),
      ],
    },
  ];

  function render() {
    const current = steps[step];
    const footer = h(
      "div",
      { class: "onboarding-footer" },
      h("span", { class: "note" }, t("onb.step", { n: step + 1, total: steps.length })),
      h(
        "div",
        { class: "row" },
        step > 0 ? h("button", { class: "ghost", onclick: () => { step -= 1; render(); } }, t("onb.back")) : null,
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
              // Clé saisie : enregistrée d'abord (une erreur garde l'écran).
              const key = keyInput.value.trim();
              if (key && !(await attempt(() => api.llmSetProviderKey(keyProvider.value, key), t("guard.key")))) return;
              // L'écran ne se ferme que si l'enregistrement a réussi.
              const done = await guard(
                () =>
                  api.completeOnboarding({
                    userName: nameInput.value.trim(),
                    model: (modelSelect as HTMLSelectElement).value,
                    voice: (voiceSelect as HTMLSelectElement).value,
                    quality: (qualitySelect as HTMLSelectElement).value,
                    language: (languageSelect as HTMLSelectElement).value,
                    grants: {
                      read: grants.read.box.checked,
                      write: grants.write.box.checked,
                      execute: grants.execute.box.checked,
                      network: grants.network.box.checked,
                    },
                  }),
                t("guard.finish"),
              );
              if (!done) return;
              ctx.status = done;
              onDone();
            },
          },
          step < steps.length - 1 ? t("onb.next") : t("onb.finish"),
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