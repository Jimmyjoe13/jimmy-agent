/** Vue Paramètres : modèle, voix, permissions, démarrage, diagnostic. */
import { api, type Settings, type StartupMode } from "../api";
import { QUALITY_LABEL, attempt, guard, h, mount, toast } from "../ui";
import type { AppContext } from "../context";
import { modelsPanel } from "./models";

export function settingsView(ctx: AppContext): HTMLElement {
  const container = h("section", { class: "view" });
  let settings: Settings | null = null;

  const modelInput = h("input", { class: "field", type: "text" }) as HTMLInputElement;
  const voiceModelInput = h("input", { class: "field", type: "text" }) as HTMLInputElement;
  const temperatureInput = h("input", {
    class: "field",
    type: "number",
    step: "0.1",
    min: "0",
    max: "2",
  }) as HTMLInputElement;
  const workspaceInput = h("input", { class: "field", type: "text" }) as HTMLInputElement;
  const voiceSelect = h("select", { class: "field" }) as HTMLSelectElement;
  const sttSelect = h("select", { class: "field" }) as HTMLSelectElement;
  const commandSelect = h("select", { class: "field" }) as HTMLSelectElement;
  const languageSelect = h("select", { class: "field" }) as HTMLSelectElement;
  const wakeInput = h("input", { class: "field", type: "text" }) as HTMLInputElement;
  const qualitySelect = h("select", { class: "field" }) as HTMLSelectElement;
  const skinSelect = h("select", { class: "field" }) as HTMLSelectElement;
  const startupSelect = h("select", { class: "field" }) as HTMLSelectElement;
  const synaptiqUrl = h("input", { class: "field", type: "text" }) as HTMLInputElement;
  const synaptiqEnabled = h("input", { type: "checkbox" }) as HTMLInputElement;
  const cuesEnabled = h("input", { type: "checkbox" }) as HTMLInputElement;
  const followSelect = h("select", { class: "field" }) as HTMLSelectElement;
  const pauseInput = h("input", { class: "field", type: "number", min: "400", max: "1500", step: "50" }) as HTMLInputElement;
  const debugAudio = h("input", { type: "checkbox" }) as HTMLInputElement;

  async function load() {
    const loaded = await guard(() => api.getSettings(), "paramètres");
    if (!loaded) return;
    settings = loaded;

    modelInput.value = settings.llm.model;
    voiceModelInput.value = settings.llm.voice_model ?? "";
    temperatureInput.value = String(settings.llm.temperature ?? "");
    workspaceInput.value = settings.workspace;

    mount(voiceSelect);
    for (const voice of ctx.status.tts.voices) {
      voiceSelect.append(h("option", { value: voice.id }, `${voice.label} — ${voice.description}`));
    }
    voiceSelect.value = settings.tts.voice;

    mount(sttSelect);
    for (const model of ctx.status.stt.models) {
      sttSelect.append(h("option", { value: model.id }, `${model.label} — ${model.note}`));
    }
    sttSelect.value = settings.stt.model;

    mount(commandSelect);
    commandSelect.append(h("option", { value: "" }, "Identique au wake word (un seul serveur)"));
    for (const model of ctx.status.stt.models) {
      commandSelect.append(h("option", { value: model.id }, `${model.label} — ${model.note}`));
    }
    commandSelect.value = settings.stt.command_model;

    mount(languageSelect);
    for (const [code, label] of LANGUAGES) {
      languageSelect.append(h("option", { value: code }, label));
    }
    languageSelect.value = settings.stt.language;
    wakeInput.value = settings.stt.wake_word;

    mount(qualitySelect);
    for (const level of ["low", "medium", "high"]) {
      qualitySelect.append(h("option", { value: level }, QUALITY_LABEL[level]));
    }
    qualitySelect.value = settings.avatar.quality;

    mount(skinSelect);
    for (const skin of ctx.status.avatar.skins) {
      skinSelect.append(h("option", { value: skin.id }, skin.label));
    }
    skinSelect.value = settings.avatar.skin;

    mount(startupSelect);
    startupSelect.append(
      h("option", { value: "manual" }, "Lancement manuel"),
      h("option", { value: "with_windows" }, "Démarrer avec Windows (visible)"),
      h("option", { value: "with_windows_hidden" }, "Démarrer avec Windows (en arrière-plan)"),
    );
    startupSelect.value = settings.startup;

    synaptiqUrl.value = settings.synaptiq.base_url;
    synaptiqEnabled.checked = settings.synaptiq.enabled;
    cuesEnabled.checked = settings.tts.cues;

    mount(followSelect);
    for (const [seconds, label] of FOLLOW_UP_CHOICES) {
      followSelect.append(h("option", { value: String(seconds) }, label));
    }
    // Une valeur personnalisée (fichier de config édité) reste sélectionnable.
    const current = Math.round(settings.voice.follow_up_ms / 1000);
    if (![...FOLLOW_UP_CHOICES].some(([s]) => s === current)) {
      followSelect.append(h("option", { value: String(current) }, `${current} s`));
    }
    followSelect.value = String(current);
    pauseInput.value = String(settings.voice.end_of_speech_ms);
    debugAudio.checked = settings.voice.debug_audio;

    render();
  }

  // Construit une seule fois : la liste, les tests et le tri survivent aux
  // re-rendus de la page (retour depuis les permissions, rechargement).
  const modelsCard = modelsPanel(ctx, {
    onApplied: (role, model) => {
      // Le choix est déjà enregistré côté Rust : les champs et la copie locale
      // suivent, sinon « Enregistrer » remettrait l'ancien modèle.
      if (role === "main") {
        modelInput.value = model;
        if (settings) settings.llm.model = model;
      } else {
        voiceModelInput.value = model;
        if (settings) settings.llm.voice_model = model;
      }
    },
  });

  async function persist() {
    if (!settings) return;
    settings.llm.model = modelInput.value.trim() || settings.llm.model;
    settings.llm.voice_model = voiceModelInput.value.trim();
    settings.llm.temperature = temperatureInput.value === "" ? null : Number(temperatureInput.value);
    settings.workspace = workspaceInput.value.trim();
    settings.tts.voice = voiceSelect.value;
    settings.stt.model = sttSelect.value;
    settings.stt.command_model = commandSelect.value;
    settings.stt.language = languageSelect.value;
    settings.stt.wake_word = wakeInput.value.trim() || "jimmy";
    settings.avatar.quality = qualitySelect.value;
    settings.avatar.skin = skinSelect.value;
    settings.synaptiq.base_url = synaptiqUrl.value.trim();
    settings.synaptiq.enabled = synaptiqEnabled.checked;
    settings.tts.cues = cuesEnabled.checked;
    settings.voice.follow_up_ms = Number(followSelect.value) * 1000;
    settings.voice.end_of_speech_ms = Math.min(1500, Math.max(400, Number(pauseInput.value) || 700));
    settings.voice.debug_audio = debugAudio.checked;

    const before = ctx.status.stt;
    if (!(await attempt(() => api.saveSettings(settings as Settings), "enregistrement"))) return;
    await ctx.refreshStatus();
    const sttChanged =
      before.model !== settings.stt.model ||
      before.command_model !== settings.stt.command_model ||
      before.language !== settings.stt.language;
    toast(
      sttChanged && ctx.voice?.running
        ? "Paramètres enregistrés — coupe et relance l'écoute (page Voix) pour appliquer les modèles."
        : "Paramètres enregistrés",
    );
  }

  async function applyStartup() {
    if (!(await attempt(() => api.setStartup(startupSelect.value as StartupMode), "démarrage automatique"))) return;
    await persist();
  }

  function render() {
    mount(
      container,
      h(
        "header",
        { class: "view-header sticky" },
        h("p", { class: "note" }, "Les changements ne sont appliqués qu'après « Enregistrer »."),
        h(
          "div",
          { class: "row" },
          h("button", { class: "ghost", onclick: () => void load() }, "Annuler"),
          h("button", { class: "primary", onclick: () => void persist() }, "Enregistrer"),
        ),
      ),
      card(
        "Modèle de langage",
        h("p", { class: "note" }, "Fournisseur : OpenCode Go (API compatible OpenAI)."),
        field("Modèle principal", modelInput, "space-bunny-free, mimo-v2.6-pro, gpt-6-luna…"),
        field("Modèle vocal (vide = le même)", voiceModelInput, "un modèle plus rapide pour les échanges à voix haute"),
        field("Température (vide = réglage du fournisseur)", temperatureInput),
        field("Dossier de travail par défaut", workspaceInput, ctx.status.workspace),
      ),
      modelsCard,
      card(
        "Voix de sortie",
        h("p", { class: "note" }, "Synthèse Fish Audio via OpenRouter. L'audio ne transite que par le réseau vers ce service."),
        field("Voix", voiceSelect),
        toggle("Sons d'état (« Oui ? » quand tu dis son nom, « Oups… » en cas d'échec)", cuesEnabled),
      ),
      card(
        "Écoute",
        h(
          "p",
          { class: "note" },
          "whisper.cpp tourne en local : l'audio n'est jamais envoyé dans le cloud.",
        ),
        field("Modèle du wake word (rapide)", sttSelect),
        field("Modèle de la commande (précis)", commandSelect),
        field("Langue", languageSelect),
        field("Mot d'activation", wakeInput, "jimmy"),
        field("Conversation continue (écoute sans redire le nom)", followSelect),
        field("Pause qui termine ta phrase (ms)", pauseInput),
        h(
          "p",
          { class: "note" },
          "Après chaque réponse, Jimmy t'écoute encore quelques secondes sans que tu aies à redire son nom. Une pause plus courte rend les réponses plus vives, mais Jimmy peut te couper si tu hésites.",
        ),
        toggle("Garder les 40 derniers extraits audio pour le diagnostic (sur cette machine seulement)", debugAudio),
      ),
      card(
        "Avatar",
        h("p", { class: "note" }, "Rendu 3D par Godot, sur un serveur HTTP local."),
        field("Qualité graphique", qualitySelect),
        field("Skin", skinSelect),
        h(
          "div",
          { class: "row" },
          h(
            "button",
            {
              class: "ghost",
              onclick: async () => {
                // Sans ce test, l'échec affichait « Avatar démarré » juste
                // après le message d'erreur.
                if (!(await attempt(() => api.avatarStart(), "avatar"))) return;
                await ctx.refreshStatus();
                toast("Avatar démarré");
              },
            },
            "Démarrer l'avatar",
          ),
          h(
            "button",
            {
              class: "ghost",
              onclick: async () => {
                if (!(await attempt(() => api.avatarStop(), "avatar"))) return;
                await ctx.refreshStatus();
                toast("Avatar arrêté");
              },
            },
            "Arrêter l'avatar",
          ),
        ),
      ),
      card(
        "Démarrage",
        field("Comportement au démarrage de Windows", startupSelect),
        h(
          "button",
          { class: "ghost", onclick: () => void applyStartup() },
          "Appliquer et enregistrer",
        ),
      ),
      card(
        "Synaptiq",
        h(
          "p",
          { class: "note" },
          "Instance locale de réflexion. Jimmy ne la consulte que lorsqu'une demande fait référence à un contexte antérieur.",
        ),
        toggle("Activer Synaptiq", synaptiqEnabled),
        field("Adresse de l'API", synaptiqUrl),
      ),
      card(
        "Permissions",
        h("p", { class: "note" }, "Jimmy agit sans redemander tant qu'une capacité est accordée."),
        h(
          "div",
          { class: "permissions" },
          ...ctx.status.permissions.map((permission) =>
            h(
              "div",
              { class: `permission ${permission.granted ? "on" : "off"}` },
              h("strong", {}, permission.capability),
              h("span", {}, permission.granted ? "accordé" : "refusé"),
            ),
          ),
        ),
        h(
          "button",
          {
            class: "ghost",
            onclick: () => {
              void openPermissions();
            },
          },
          "Ajuster les permissions",
        ),
      ),
    );
  }

  async function openPermissions() {
    const permissions = await guard(() => api.getPermissions(), "permissions");
    if (!permissions) return;
    const keys: (keyof typeof permissions)[] = ["read", "write", "execute", "network"];
    const rows = keys.map((key) => {
      const rule = permissions[key];
      const checkbox = h("input", { type: "checkbox" }) as HTMLInputElement;
      checkbox.checked = rule.granted;
      const label = h("span", {}, rule.granted ? "accordé" : "refusé");
      checkbox.addEventListener("change", () => {
        rule.granted = checkbox.checked;
        // Le libellé suivait l'état initial et ne changeait jamais.
        label.textContent = checkbox.checked ? "accordé" : "refusé";
      });
      return h(
        "div",
        { class: "permission-row" },
        h("strong", {}, key),
        h("label", { class: "toggle" }, checkbox, label),
        h(
          "details",
          {},
          h("summary", {}, "motifs autorisés"),
          h(
            "div",
            { class: "note pre" },
            `chemins : ${rule.allow_paths.join(", ") || "tous"}\ninterdits : ${rule.deny_commands.join(", ") || "aucun"}`,
          ),
        ),
      );
    });
    mount(
      container,
      h(
        "header",
        { class: "view-header" },
        h("button", { class: "ghost", onclick: () => render() }, "← Retour"),
        h("h2", {}, "Permissions"),
      ),
      card(
        "Capacités",
        h(
          "p",
          { class: "note" },
          "Une capacité non accordée produit un refus lisible que Jimmy reçoit et peut expliquer.",
        ),
        ...rows,
        h(
          "button",
          {
            class: "primary",
            onclick: async () => {
              if (!(await attempt(() => api.savePermissions(permissions), "permissions"))) return;
              await ctx.refreshStatus();
              render();
              toast("Permissions enregistrées");
            },
          },
          "Enregistrer",
        ),
      ),
    );
  }

  void load();
  return container;
}

/** Durées proposées pour la conversation continue : [secondes, libellé]. */
const FOLLOW_UP_CHOICES: [number, string][] = [
  [0, "Désactivée (dire « Jimmy » à chaque phrase)"],
  [5, "5 secondes"],
  [8, "8 secondes (recommandé)"],
  [12, "12 secondes"],
  [20, "20 secondes"],
];

/** Langues proposées pour la reconnaissance vocale (codes Whisper). */
const LANGUAGES: [string, string][] = [
  ["fr", "Français"],
  ["en", "Anglais"],
  ["es", "Espagnol"],
  ["de", "Allemand"],
  ["it", "Italien"],
  ["auto", "Détection automatique"],
];

export function card(title: string, ...children: (Node | string | null | false)[]): HTMLElement {
  return h("section", { class: "card" }, h("h3", {}, title), ...children);
}

export function field(label: string, input: HTMLElement, placeholder = ""): HTMLElement {
  if (placeholder) input.setAttribute("placeholder", placeholder);
  return h("label", { class: "field-row" }, h("span", {}, label), input);
}

export function toggle(label: string, input: HTMLElement): HTMLElement {
  return h("label", { class: "toggle" }, input, h("span", {}, label));
}