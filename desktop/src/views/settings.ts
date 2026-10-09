/** Vue Paramètres : deux sous-onglets — Général (voix, écoute, avatar…) et
 *  LLM (fournisseurs, modèles, réglages du modèle de langage). */
import { api, type Settings, type StartupMode } from "../api";
import { QUALITY_LABEL, attempt, guard, h, mount, toast } from "../ui";
import type { AppContext } from "../context";
import { modelsPanel } from "./models";
import { providersPanel } from "./providers";

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
  const maxTokensInput = h("input", { class: "field", type: "number", min: "1024", step: "1024" }) as HTMLInputElement;
  const maxIterationsInput = h("input", { class: "field", type: "number", min: "1", max: "100" }) as HTMLInputElement;
  const workspaceInput = h("input", { class: "field", type: "text" }) as HTMLInputElement;
  const qualitySelect = h("select", { class: "field" }) as HTMLSelectElement;
  const skinSelect = h("select", { class: "field" }) as HTMLSelectElement;
  const startupSelect = h("select", { class: "field" }) as HTMLSelectElement;
  const vaultPathInput = h("input", { class: "field", type: "text" }) as HTMLInputElement;
  const vaultFolderInput = h("input", { class: "field", type: "text" }) as HTMLInputElement;
  const vaultEnabled = h("input", { type: "checkbox" }) as HTMLInputElement;
  const memoryModelInput = h("input", {
    class: "field",
    type: "text",
    placeholder: "Vide = modèle principal",
  }) as HTMLInputElement;

  async function load() {
    const loaded = await guard(() => api.getSettings(), "paramètres");
    if (!loaded) return;
    settings = loaded;

    modelInput.value = settings.llm.model;
    temperatureInput.value = String(settings.llm.temperature ?? "");
    maxTokensInput.value = String(settings.llm.max_tokens);
    maxIterationsInput.value = String(settings.llm.max_iterations);
    workspaceInput.value = settings.workspace;

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

    vaultPathInput.value = settings.memory.vault_path;
    // Vide = automatique : on montre le vault réellement ouvert.
    const origin: Record<string, string> = { obsidian: "vault d'Obsidian", own: "vault propre de Jimy" };
    const detected = ctx.status.vault.path;
    vaultPathInput.placeholder = detected
      ? `Automatique — ${detected} (${origin[ctx.status.vault.origin] ?? "réglé"})`
      : "Automatique (vault d'Obsidian, sinon vault propre de Jimy)";
    vaultFolderInput.value = settings.memory.vault_folder;
    vaultEnabled.checked = settings.memory.vault_enabled;
    memoryModelInput.value = settings.memory.model ?? "";

    render();
  }

  // Construit une seule fois : la liste, les tests et le tri survivent aux
  // re-rendus de la page (retour depuis les permissions, rechargement).
  // Restreint au rôle principal : le modèle vocal se choisit dans l'onglet Voix.
  const modelsCard = modelsPanel(ctx, {
    onApplied: (role, model, provider) => {
      // Le choix est déjà enregistré côté Rust : les champs et la copie locale
      // suivent, sinon « Enregistrer » remettrait l'ancien modèle.
      if (role === "main") {
        modelInput.value = model;
        if (settings) settings.llm.model = model;
        if (settings && provider) settings.llm.provider = provider;
      }
    },
  }, "main");
  const providersCard = providersPanel(ctx, {
    // Fournisseur ajouté, retiré ou modifié : la copie locale de la vue doit
    // rester à jour, sinon « Enregistrer » écraserait le changement.
    onChanged: () => {
      void load();
    },
  });

  async function persist() {
    if (!settings) return;
    settings.llm.model = modelInput.value.trim() || settings.llm.model;
    settings.llm.temperature = temperatureInput.value === "" ? null : Number(temperatureInput.value);
    settings.llm.max_tokens = Math.max(1024, Number(maxTokensInput.value) || settings.llm.max_tokens);
    settings.llm.max_iterations = Math.min(100, Math.max(1, Number(maxIterationsInput.value) || settings.llm.max_iterations));
    settings.workspace = workspaceInput.value.trim();
    // Les fournisseurs ET tout le vocal (modèle vocal, STT, écoute, sons
    // d'état) se règlent ailleurs (carte Fournisseurs, onglet Voix, tous à
    // enregistrement immédiat ou propre formulaire) : on repart de la copie
    // fraîche de Rust pour ne rien écraser d'un formulaire resté ouvert
    // pendant ce temps. Les clés masquées sont restaurées côté Rust (une clé
    // vide = inchangée).
    const fresh = await guard(() => api.getSettings(), "fournisseurs");
    if (fresh) {
      settings.llm.providers = fresh.llm.providers;
      settings.llm.voice_model = fresh.llm.voice_model;
      settings.llm.voice_provider = fresh.llm.voice_provider;
      settings.stt = fresh.stt;
      settings.voice = fresh.voice;
      settings.tts.cues = fresh.tts.cues;
    }
    settings.avatar.quality = qualitySelect.value;
    settings.avatar.skin = skinSelect.value;
    settings.memory.vault_path = vaultPathInput.value.trim();
    settings.memory.vault_folder = vaultFolderInput.value.trim() || "0_Inbox/Jimmy";
    settings.memory.vault_enabled = vaultEnabled.checked;
    settings.memory.model = memoryModelInput.value.trim();

    if (!(await attempt(() => api.saveSettings(settings as Settings), "enregistrement"))) return;
    await ctx.refreshStatus();
    toast("Paramètres enregistrés");
  }

  async function applyStartup() {
    if (!(await attempt(() => api.setStartup(startupSelect.value as StartupMode), "démarrage automatique"))) return;
    await persist();
  }

  // Deux sous-onglets, comme Skills → Serveurs MCP : tout ce qui touche au
  // modèle de langage (fournisseurs, modèles, budget) est dans « LLM ».
  const generalPane = h("div", {});
  const llmPane = h("div", {});
  let tab: "general" | "llm" = "general";
  const tabButtons = {
    general: h("button", { class: "subtab", role: "tab", onclick: () => select("general") }, "Général"),
    llm: h("button", { class: "subtab", role: "tab", onclick: () => select("llm") }, "LLM"),
  };

  function select(next: "general" | "llm") {
    tab = next;
    for (const [key, button] of Object.entries(tabButtons)) {
      const active = key === tab;
      button.classList.toggle("active", active);
      button.setAttribute("aria-selected", String(active));
    }
    generalPane.hidden = tab !== "general";
    llmPane.hidden = tab !== "llm";
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
      h("div", { class: "subtabs", role: "tablist" }, tabButtons.general, tabButtons.llm),
      generalPane,
      llmPane,
    );
    mount(
      generalPane,
      card(
        "Dossier de travail",
        field("Dossier de travail par défaut", workspaceInput, ctx.status.workspace),
      ),
      card(
        "Voix et écoute",
        h("p", { class: "note" }, "Tout ce qui touche à la voix de Jimy (modèle vocal, synthèse, écoute, mot d'activation) se règle dans l'onglet ", h("strong", {}, "Voix"), "."),
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
        "Vault Obsidian",
        h(
          "p",
          { class: "note" },
          "Mémoire persistante de Jimy : ses souvenirs sont des notes Markdown dans ton vault, que tu relis comme les autres. Jimy lit tout le vault et écrit dans son dossier. Chemin vide = automatique (le vault d'Obsidian de la machine, sinon le sien, rangé en PARA).",
        ),
        toggle("Connecter le vault Obsidian", vaultEnabled),
        field("Chemin du vault", vaultPathInput),
        field("Dossier des souvenirs de Jimy", vaultFolderInput),
        field("Modèle de la mémoire (apprentissage et tri du vault)", memoryModelInput),
      ),
      card(
        "Permissions",
        h("p", { class: "note" }, "Jimy agit sans redemander tant qu'une capacité est accordée."),
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
    mount(
      llmPane,
      providersCard,
      card(
        "Modèle de langage",
        h("p", { class: "note" }, "Le modèle principal sert au Chat et aux tâches — choisis-le dans la bibliothèque ci-dessous, ou tape l'identifiant ici. Le modèle vocal (échanges à voix haute) se choisit dans l'onglet Voix."),
        field("Modèle principal", modelInput, "space-bunny-free, mimo-v2.6-pro, deepseek-chat…"),
        field("Température (vide = réglage du fournisseur)", temperatureInput),
      ),
      modelsCard,
      card(
        "Budget de l'agent",
        h("p", { class: "note" }, "Limites d'une seule demande. Trop bas, Jimy s'arrête avant d'avoir fini ; trop haut, une dérive coûte cher."),
        field("Jetons maximum par réponse", maxTokensInput),
        h(
          "p",
          { class: "note" },
          "Le raisonnement caché d'un modèle compte dans ce budget : 16 384 est le minimum confortable (mesuré le 4 octobre).",
        ),
        field("Allers-retours maximum avec les outils", maxIterationsInput),
      ),
    );
    select(tab);
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
          "Une capacité non accordée produit un refus lisible que Jimy reçoit et peut expliquer.",
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
export const FOLLOW_UP_CHOICES: [number, string][] = [
  [0, "Désactivée (dire « Jimy » à chaque phrase)"],
  [5, "5 secondes"],
  [8, "8 secondes (recommandé)"],
  [12, "12 secondes"],
  [20, "20 secondes"],
];

/** Langues proposées pour la reconnaissance vocale (codes Whisper). */
export const LANGUAGES: [string, string][] = [
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
