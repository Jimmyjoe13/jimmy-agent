/** Vue Paramètres : modèle, voix, permissions, démarrage, diagnostic. */
import { api, type Settings, type StartupMode } from "../api";
import { attempt, guard, h, mount, toast } from "../ui";
import type { AppContext } from "../context";

export function settingsView(ctx: AppContext): HTMLElement {
  const container = h("section", { class: "view" });
  let settings: Settings | null = null;

  const modelInput = h("input", { class: "field", type: "text" }) as HTMLInputElement;
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
  const languageInput = h("input", { class: "field", type: "text" }) as HTMLInputElement;
  const wakeInput = h("input", { class: "field", type: "text" }) as HTMLInputElement;
  const qualitySelect = h("select", { class: "field" }) as HTMLSelectElement;
  const skinSelect = h("select", { class: "field" }) as HTMLSelectElement;
  const startupSelect = h("select", { class: "field" }) as HTMLSelectElement;
  const synaptiqUrl = h("input", { class: "field", type: "text" }) as HTMLInputElement;
  const synaptiqEnabled = h("input", { type: "checkbox" }) as HTMLInputElement;
  const cuesEnabled = h("input", { type: "checkbox" }) as HTMLInputElement;

  async function load() {
    const loaded = await guard(() => api.getSettings(), "paramètres");
    if (!loaded) return;
    settings = loaded;

    modelInput.value = settings.llm.model;
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

    languageInput.value = settings.stt.language;
    wakeInput.value = settings.stt.wake_word;

    mount(qualitySelect);
    for (const level of ["low", "medium", "high"]) {
      qualitySelect.append(h("option", { value: level }, level));
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

    render();
  }

  async function persist() {
    if (!settings) return;
    settings.llm.model = modelInput.value.trim() || settings.llm.model;
    settings.llm.temperature = temperatureInput.value === "" ? null : Number(temperatureInput.value);
    settings.workspace = workspaceInput.value.trim();
    settings.tts.voice = voiceSelect.value;
    settings.stt.model = sttSelect.value;
    settings.stt.command_model = commandSelect.value;
    settings.stt.language = languageInput.value.trim();
    settings.stt.wake_word = wakeInput.value.trim() || "jimmy";
    settings.avatar.quality = qualitySelect.value;
    settings.avatar.skin = skinSelect.value;
    settings.synaptiq.base_url = synaptiqUrl.value.trim();
    settings.synaptiq.enabled = synaptiqEnabled.checked;
    settings.tts.cues = cuesEnabled.checked;

    await guard(() => api.saveSettings(settings as Settings), "enregistrement");
    await ctx.refreshStatus();
    toast("Paramètres enregistrés");
  }

  async function applyStartup() {
    await guard(() => api.setStartup(startupSelect.value as StartupMode), "démarrage automatique");
    await persist();
  }

  function render() {
    mount(
      container,
      h(
        "header",
        { class: "view-header" },
        h("h2", {}, "Paramètres"),
        h(
          "div",
          { class: "row" },
          h("button", { class: "ghost", onclick: () => void load() }, "Recharger"),
          h("button", { class: "primary", onclick: () => void persist() }, "Enregistrer"),
        ),
      ),
      card(
        "Modèle de langage",
        h("p", { class: "note" }, "Fournisseur : OpenCode Go (API compatible OpenAI)."),
        field("Modèle", modelInput, "space-bunny-free, mimo-v2.6-pro, gpt-6-luna…"),
        field("Température (vide = réglage du fournisseur)", temperatureInput),
        field("Dossier de travail par défaut", workspaceInput, ctx.status.workspace),
      ),
      card(
        "Voix de sortie",
        h("p", { class: "note" }, "Synthèse Fish Audio via OpenRouter. L'audio ne transite que par le réseau vers ce service."),
        field("Voix", voiceSelect),
        toggle("Sons d'état (« Oui ? », « C'est prêt. », « Oups… »)", cuesEnabled),
      ),
      card(
        "Écoute",
        h(
          "p",
          { class: "note" },
          "whisper.cpp tourne en local : l'audio n'est jamais envoyé dans le cloud.",
        ),
        field("Modèle du wake word (rapide)", sttSelect),
        field("Modèle de la commande (précis, au prochain démarrage de l'écoute)", commandSelect),
        field("Langue", languageInput, "fr"),
        field("Mot d'activation", wakeInput, "jimmy"),
      ),
      card(
        "Avatar",
        h("p", { class: "note" }, "Rendu 3D par Godot, sur un serveur HTTP local."),
        field("Qualité graphique", qualitySelect, "low / medium / high"),
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
                await guard(() => api.avatarStop(), "avatar");
                await ctx.refreshStatus();
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
      checkbox.addEventListener("change", () => {
        rule.granted = checkbox.checked;
      });
      return h(
        "div",
        { class: "permission-row" },
        h("strong", {}, key),
        h("label", { class: "toggle" }, checkbox, h("span", {}, rule.granted ? "accordé" : "refusé")),
        h(
          "details",
          {},
          h("summary", {}, "motifs autorisés"),
          h(
            "div",
            { class: "note" },
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
              await guard(() => api.savePermissions(permissions), "permissions");
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