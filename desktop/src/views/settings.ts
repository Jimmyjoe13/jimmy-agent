/** Vue Paramètres : trois sous-onglets — Général (voix, écoute, avatar…),
 *  LLM (fournisseurs, modèles, réglages du modèle de langage) et Âme
 *  (le SOUL.md de Jimy, `soul.ts`). */
import { api, type Settings, type StartupMode } from "../api";
import { attempt, guard, h, mount, qualityLabel, toast } from "../ui";
import type { AppContext } from "../context";
import { setLanguage, t, type Key } from "../i18n";
import { modelsPanel } from "./models";
import { providersPanel } from "./providers";
import { soulPanel } from "./soul";

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
  const shareWindowTitle = h("input", { type: "checkbox" }) as HTMLInputElement;
  const memoryModelInput = h("input", {
    class: "field",
    type: "text",
    placeholder: t("settings.memoryModelPlaceholder"),
  }) as HTMLInputElement;
  const memoryProviderSelect = h("select", { class: "field" }) as HTMLSelectElement;

  async function load() {
    const loaded = await guard(() => api.getSettings(), t("guard.settings"));
    if (!loaded) return;
    settings = loaded;

    modelInput.value = settings.llm.model;
    temperatureInput.value = String(settings.llm.temperature ?? "");
    maxTokensInput.value = String(settings.llm.max_tokens);
    maxIterationsInput.value = String(settings.llm.max_iterations);
    workspaceInput.value = settings.workspace;

    mount(qualitySelect);
    for (const level of ["low", "medium", "high"]) {
      qualitySelect.append(h("option", { value: level }, qualityLabel(level)));
    }
    qualitySelect.value = settings.avatar.quality;

    mount(skinSelect);
    for (const skin of ctx.status.avatar.skins) {
      skinSelect.append(h("option", { value: skin.id }, skin.label));
    }
    // Skins premium : seulement ceux qu'une licence débloque.
    for (const skin of ctx.status.avatar.premium.filter((s) => s.unlocked)) {
      skinSelect.append(h("option", { value: skin.id }, skin.label));
    }
    skinSelect.value = settings.avatar.skin;

    mount(startupSelect);
    startupSelect.append(
      h("option", { value: "manual" }, t("settings.startup.manual")),
      h("option", { value: "with_windows" }, t("settings.startup.visible")),
      h("option", { value: "with_windows_hidden" }, t("settings.startup.hidden")),
    );
    startupSelect.value = settings.startup;

    vaultPathInput.value = settings.memory.vault_path;
    // Vide = automatique : on montre le vault réellement ouvert.
    const origin: Record<string, string> = {
      obsidian: t("settings.vaultOrigin.obsidian"),
      own: t("settings.vaultOrigin.own"),
    };
    const detected = ctx.status.vault.path;
    vaultPathInput.placeholder = detected
      ? t("settings.vaultAutoDetected", { path: detected, origin: origin[ctx.status.vault.origin] ?? t("settings.vaultOrigin.configured") })
      : t("settings.vaultAutoFallback");
    vaultFolderInput.value = settings.memory.vault_folder;
    vaultEnabled.checked = settings.memory.vault_enabled;
    shareWindowTitle.checked = settings.memory.share_window_title;
    memoryModelInput.value = settings.memory.model ?? "";
    // Fournisseur de la mémoire : vide = celui du principal (14 octobre).
    mount(memoryProviderSelect);
    memoryProviderSelect.append(h("option", { value: "" }, t("settings.memoryProviderSame")));
    for (const provider of settings.llm.providers) {
      if (!provider.enabled) continue;
      memoryProviderSelect.append(h("option", { value: provider.id }, provider.label));
    }
    const currentMemoryProvider = settings.llm.memory_provider ?? "";
    if (currentMemoryProvider && ![...memoryProviderSelect.options].some((o) => o.value === currentMemoryProvider)) {
      memoryProviderSelect.append(h("option", { value: currentMemoryProvider }, t("settings.providerDisabled", { id: currentMemoryProvider })));
    }
    memoryProviderSelect.value = currentMemoryProvider;

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
    const fresh = await guard(() => api.getSettings(), t("guard.providers"));
    if (fresh) {
      settings.llm.providers = fresh.llm.providers;
      settings.llm.voice_model = fresh.llm.voice_model;
      settings.llm.voice_provider = fresh.llm.voice_provider;
      settings.stt = fresh.stt;
      settings.voice = fresh.voice;
      settings.tts = fresh.tts;
    }
    settings.avatar.quality = qualitySelect.value;
    settings.avatar.skin = skinSelect.value;
    settings.memory.vault_path = vaultPathInput.value.trim();
    settings.memory.vault_folder = vaultFolderInput.value.trim() || "0_Inbox/Jimmy";
    settings.memory.vault_enabled = vaultEnabled.checked;
    settings.memory.share_window_title = shareWindowTitle.checked;
    settings.memory.model = memoryModelInput.value.trim();
    settings.llm.memory_provider = memoryProviderSelect.value;

    if (!(await attempt(() => api.saveSettings(settings as Settings), t("guard.save")))) return;
    await ctx.refreshStatus();
    toast(t("settings.saved"));
  }

  async function applyStartup() {
    if (!(await attempt(() => api.setStartup(startupSelect.value as StartupMode), t("guard.startup")))) return;
    await persist();
  }

  // Trois sous-onglets, comme Skills → Serveurs MCP : tout ce qui touche au
  // modèle de langage (fournisseurs, modèles, budget) est dans « LLM » ;
  // « Âme » montre le SOUL.md de Jimy, qu'il fait évoluer lui-même. Son
  // panneau s'enregistre seul (indépendant du bouton « Enregistrer » du haut).
  const generalPane = h("div", {});
  const llmPane = h("div", {});
  const soulPane = soulPanel();
  let tab: "general" | "llm" | "soul" = "general";
  const tabButtons = {
    general: h("button", { class: "subtab", role: "tab", onclick: () => select("general") }, t("settings.tab.general")),
    llm: h("button", { class: "subtab", role: "tab", onclick: () => select("llm") }, t("settings.tab.llm")),
    soul: h("button", { class: "subtab", role: "tab", onclick: () => select("soul") }, t("settings.tab.soul")),
  };

  function select(next: "general" | "llm" | "soul") {
    tab = next;
    for (const [key, button] of Object.entries(tabButtons)) {
      const active = key === tab;
      button.classList.toggle("active", active);
      button.setAttribute("aria-selected", String(active));
    }
    generalPane.hidden = tab !== "general";
    llmPane.hidden = tab !== "llm";
    soulPane.hidden = tab !== "soul";
  }

  function render() {
    mount(
      container,
      h(
        "header",
        { class: "view-header sticky" },
        h("p", { class: "note" }, t("settings.headerNote")),
        h(
          "div",
          { class: "row" },
          h("button", { class: "ghost", onclick: () => void load() }, t("settings.cancel")),
          h("button", { class: "primary", onclick: () => void persist() }, t("settings.save")),
        ),
      ),
      languageCard(),
      h("div", { class: "subtabs", role: "tablist" }, tabButtons.general, tabButtons.llm, tabButtons.soul),
      generalPane,
      llmPane,
      soulPane,
    );
    mount(
      generalPane,
      card(
        t("settings.workspace.title"),
        field(t("settings.workspace.label"), workspaceInput, ctx.status.workspace),
      ),
      card(
        t("settings.voiceCard.title"),
        h("p", { class: "note" }, t("settings.voiceCard.noteBefore"), h("strong", {}, t("settings.voiceCard.noteTab")), "."),
      ),
      card(
        t("settings.avatar.title"),
        h("p", { class: "note" }, t("settings.avatar.note")),
        field(t("settings.avatar.quality"), qualitySelect),
        field(t("settings.avatar.skin"), skinSelect),
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
                if (!(await attempt(() => api.avatarStart(), t("guard.avatar")))) return;
                await ctx.refreshStatus();
                toast(t("settings.avatar.started"));
              },
            },
            t("settings.avatar.start"),
          ),
          h(
            "button",
            {
              class: "ghost",
              onclick: async () => {
                if (!(await attempt(() => api.avatarStop(), t("guard.avatar")))) return;
                await ctx.refreshStatus();
                toast(t("settings.avatar.stopped"));
              },
            },
            t("settings.avatar.stop"),
          ),
        ),
      ),
      card(
        t("settings.startup.title"),
        field(t("settings.startup.label"), startupSelect),
        h(
          "button",
          { class: "ghost", onclick: () => void applyStartup() },
          t("settings.startup.apply"),
        ),
      ),
      card(
        t("settings.vault.title"),
        h(
          "p",
          { class: "note" },
          t("settings.vault.note"),
        ),
        toggle(t("settings.vault.enable"), vaultEnabled),
        field(t("settings.vault.path"), vaultPathInput),
        field(t("settings.vault.folder"), vaultFolderInput),
        field(t("settings.vault.model"), memoryModelInput),
        field(t("settings.vault.provider"), memoryProviderSelect),
      ),
      card(
        t("settings.context.title"),
        h(
          "p",
          { class: "note" },
          t("settings.context.note"),
        ),
        toggle(t("settings.context.shareWindow"), shareWindowTitle),
      ),
      card(
        t("settings.permissions.title"),
        h("p", { class: "note" }, t("settings.permissions.note")),
        h(
          "div",
          { class: "permissions" },
          ...ctx.status.permissions.map((permission) =>
            h(
              "div",
              { class: `permission ${permission.granted ? "on" : "off"}` },
              h("strong", {}, permission.capability),
              h("span", {}, permission.granted ? t("settings.permissions.granted") : t("settings.permissions.denied")),
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
          t("settings.permissions.adjust"),
        ),
      ),
    );
    const brainTestStatus = h("span", { class: "brain-test-status" });
    const brainTestButton = h(
      "button",
      {
        class: "ghost brain-test-now",
        onclick: async () => {
          brainTestButton.setAttribute("disabled", "");
          brainTestStatus.className = "brain-test-status";
          brainTestStatus.textContent = t("settings.brain.testing");
          try {
            const res = await api.llmTestModel(ctx.status.llm.model, ctx.status.llm.provider);
            const s = (res.latency_ms / 1000).toFixed(1).replace(".", ",");
            if (res.ok && res.tools) {
              brainTestStatus.className = "brain-test-status ok";
              brainTestStatus.textContent = t("settings.brain.testOk", { s });
            } else if (res.ok) {
              brainTestStatus.className = "brain-test-status warn";
              brainTestStatus.textContent = t("settings.brain.testWarn", { s });
            } else {
              brainTestStatus.className = "brain-test-status bad";
              brainTestStatus.textContent = t("settings.brain.testFail", { error: res.error });
            }
          } catch (error) {
            brainTestStatus.className = "brain-test-status bad";
            brainTestStatus.textContent = t("settings.brain.testFail", { error: String(error) });
          }
          brainTestButton.removeAttribute("disabled");
        },
      },
      t("settings.brain.testNow"),
    );
    const activeProviderLabel =
      ctx.status.llm.providers?.find((p) => p.id === ctx.status.llm.provider)?.label ?? ctx.status.llm.provider;
    const brainHero = h(
      "section",
      { class: "card brain-hero", "aria-label": t("settings.brain.kicker") },
      h(
        "div",
        { class: "brain-hero-main" },
        h("span", { class: "kicker" }, t("settings.brain.kicker")),
        h("strong", { class: "brain-hero-model" }, ctx.status.llm.model),
        h("span", { class: "note" }, t("settings.brain.atProvider"), h("strong", {}, activeProviderLabel)),
      ),
      h("div", { class: "brain-hero-actions" }, brainTestStatus, brainTestButton),
    );

    const singleBrainBox = h("input", { type: "checkbox", class: "brain-single-box" }) as HTMLInputElement;
    const isSingleBrain = !(settings?.llm.voice_model || "").trim() && !(settings?.memory.model || "").trim();
    singleBrainBox.checked = isSingleBrain;
    const rolesSummary = h(
      "div",
      { class: "brain-roles-grid" },
      h(
        "div",
        { class: "brain-role-tile" },
        h("strong", {}, t("settings.brain.roleChat")),
        h("span", { class: "note" }, t("settings.brain.roleChatHint")),
        h("code", {}, ctx.status.llm.model),
      ),
      h(
        "div",
        { class: "brain-role-tile" },
        h("strong", {}, t("settings.brain.roleVoice")),
        h("span", { class: "note" }, t("settings.brain.roleVoiceHint")),
        h("code", {}, ctx.status.llm.voice_model || ctx.status.llm.model),
      ),
      h(
        "div",
        { class: "brain-role-tile" },
        h("strong", {}, t("settings.brain.roleMemory")),
        h("span", { class: "note" }, t("settings.brain.roleMemoryHint")),
        h("code", {}, settings?.memory.model || ctx.status.llm.model),
      ),
    );
    rolesSummary.hidden = singleBrainBox.checked;
    singleBrainBox.addEventListener("change", async () => {
      rolesSummary.hidden = singleBrainBox.checked;
      if (singleBrainBox.checked && settings) {
        await attempt(() => api.setLlmModel("voice", ""), t("guard.voiceModel"));
        memoryModelInput.value = "";
        memoryProviderSelect.value = "";
        await persist();
      }
    });
    const singleBrainCard = h(
      "section",
      { class: "card brain-single-card" },
      toggle(t("settings.brain.singleTitle"), singleBrainBox),
      h("p", { class: "note" }, t("settings.brain.singleHint")),
      rolesSummary,
    );

    mount(
      llmPane,
      brainHero,
      singleBrainCard,
      providersCard,
      card(
        t("settings.llm.title"),
        h("p", { class: "note" }, t("settings.llm.note")),
        field(t("settings.llm.main"), modelInput, "space-bunny-free, mimo-v2.6-pro, deepseek-chat…"),
        field(t("settings.llm.temperature"), temperatureInput),
      ),
      modelsCard,
      card(
        t("settings.budget.title"),
        h("p", { class: "note" }, t("settings.budget.note")),
        field(t("settings.budget.maxTokens"), maxTokensInput),
        h(
          "p",
          { class: "note" },
          t("settings.budget.maxTokensNote"),
        ),
        field(t("settings.budget.maxIter"), maxIterationsInput),
      ),
    );
    select(tab);
  }

  async function openPermissions() {
    const permissions = await guard(() => api.getPermissions(), t("guard.permissions"));
    if (!permissions) return;
    // Accords « Toujours autoriser » donnés depuis les cartes du Chat ou d'ici.
    const always = (await guard(() => api.approvalsAlways(), t("guard.alwaysApprovals"))) ?? [];
    const alwaysList = h("div", { class: "always-list" });
    const POWER_SCOPE = "machine : éteindre ou redémarrer l'ordinateur";
    const POWER_DENIES = ["shutdown*", "reboot*", "halt*", "poweroff*"];
    const powerSelect = h(
      "select",
      { class: "field power-mode-select" },
      h("option", { value: "ask" }, t("settings.perm.mode.ask")),
      h("option", { value: "always" }, t("settings.perm.mode.always")),
      h("option", { value: "deny" }, t("settings.perm.mode.deny")),
    ) as HTMLSelectElement;

    const currentPowerMode = (): "ask" | "always" | "deny" => {
      if (permissions.execute.deny_commands.some((m) => POWER_DENIES.includes(m.toLowerCase()))) {
        return "deny";
      }
      if (always.includes(POWER_SCOPE)) {
        return "always";
      }
      return "ask";
    };
    powerSelect.value = currentPowerMode();

    const renderAlways = () => {
      if (!always.length) {
        mount(alwaysList, h("p", { class: "note" }, t("settings.perm.alwaysNone")));
        return;
      }
      mount(
        alwaysList,
        ...always.map((key) =>
          h(
            "div",
            { class: "permission-row always-row" },
            h("code", {}, key),
            h(
              "button",
              {
                class: "ghost small",
                onclick: async () => {
                  if (!(await attempt(() => api.approvalRevoke(key), t("guard.approval")))) return;
                  always.splice(always.indexOf(key), 1);
                  powerSelect.value = currentPowerMode();
                  webcamSelect.value = currentWebcamMode();
                  renderAlways();
                  toast(t("settings.perm.alwaysRevoked"));
                },
              },
              t("settings.perm.remove"),
            ),
          ),
        ),
      );
    };
    renderAlways();

    powerSelect.addEventListener("change", async () => {
      const mode = powerSelect.value as "ask" | "always" | "deny";
      permissions.execute.deny_commands = permissions.execute.deny_commands.filter(
        (m) => !POWER_DENIES.includes(m.toLowerCase()),
      );
      if (mode === "deny") {
        permissions.execute.deny_commands.push("shutdown*");
      }
      if (!(await attempt(() => api.savePermissions(permissions), t("guard.permissions")))) return;
      if (mode === "always") {
        if (!always.includes(POWER_SCOPE)) {
          if (!(await attempt(() => api.approvalGrant(POWER_SCOPE), t("guard.approval")))) return;
          always.push(POWER_SCOPE);
          always.sort();
        }
      } else if (always.includes(POWER_SCOPE)) {
        if (!(await attempt(() => api.approvalRevoke(POWER_SCOPE), t("guard.approval")))) return;
        always.splice(always.indexOf(POWER_SCOPE), 1);
      }
      renderAlways();
      toast(
        mode === "always"
          ? t("settings.perm.power.always")
          : mode === "deny"
            ? t("settings.perm.power.deny")
            : t("settings.perm.power.ask"),
      );
    });

    // Webcam (Lot 2) : même modèle à 3 modes que l'extinction — carte par
    // défaut (vie privée), « Toujours » sans redemander, « Interdit » via
    // `deny_commands` (`webcam*`).
    const WEBCAM_SCOPE = "machine : voir via la webcam";
    const WEBCAM_DENIES = ["webcam*"];
    const webcamSelect = h(
      "select",
      { class: "field webcam-mode-select" },
      h("option", { value: "ask" }, t("settings.perm.mode.ask")),
      h("option", { value: "always" }, t("settings.perm.mode.always")),
      h("option", { value: "deny" }, t("settings.perm.mode.deny")),
    ) as HTMLSelectElement;

    const currentWebcamMode = (): "ask" | "always" | "deny" => {
      if (permissions.execute.deny_commands.some((m) => WEBCAM_DENIES.includes(m.toLowerCase()))) {
        return "deny";
      }
      if (always.includes(WEBCAM_SCOPE)) {
        return "always";
      }
      return "ask";
    };
    webcamSelect.value = currentWebcamMode();

    webcamSelect.addEventListener("change", async () => {
      const mode = webcamSelect.value as "ask" | "always" | "deny";
      permissions.execute.deny_commands = permissions.execute.deny_commands.filter(
        (m) => !WEBCAM_DENIES.includes(m.toLowerCase()),
      );
      if (mode === "deny") {
        permissions.execute.deny_commands.push("webcam*");
      }
      if (!(await attempt(() => api.savePermissions(permissions), t("guard.permissions")))) return;
      if (mode === "always") {
        if (!always.includes(WEBCAM_SCOPE)) {
          if (!(await attempt(() => api.approvalGrant(WEBCAM_SCOPE), t("guard.approval")))) return;
          always.push(WEBCAM_SCOPE);
          always.sort();
        }
      } else if (always.includes(WEBCAM_SCOPE)) {
        if (!(await attempt(() => api.approvalRevoke(WEBCAM_SCOPE), t("guard.approval")))) return;
        always.splice(always.indexOf(WEBCAM_SCOPE), 1);
      }
      powerSelect.value = currentPowerMode();
      renderAlways();
      toast(
        mode === "always"
          ? t("settings.perm.webcam.always")
          : mode === "deny"
            ? t("settings.perm.webcam.deny")
            : t("settings.perm.webcam.ask"),
      );
    });
    const keys: (keyof typeof permissions)[] = ["read", "write", "execute", "network"];
    const rows = keys.map((key) => {
      const rule = permissions[key];
      const checkbox = h("input", { type: "checkbox" }) as HTMLInputElement;
      checkbox.checked = rule.granted;
      const label = h("span", {}, rule.granted ? t("settings.permissions.granted") : t("settings.permissions.denied"));
      checkbox.addEventListener("change", () => {
        rule.granted = checkbox.checked;
        // Le libellé suivait l'état initial et ne changeait jamais.
        label.textContent = checkbox.checked ? t("settings.permissions.granted") : t("settings.permissions.denied");
      });
      return h(
        "div",
        { class: "permission-row" },
        h("strong", {}, key),
        h("label", { class: "toggle" }, checkbox, label),
        h(
          "details",
          {},
          h("summary", {}, t("settings.perm.allowedPatterns")),
          h(
            "div",
            { class: "note pre" },
            t("settings.perm.pathsAndDenies", {
              paths: rule.allow_paths.join(", ") || t("settings.perm.allPaths"),
              denies: rule.deny_commands.join(", ") || t("settings.perm.noDenies"),
            }),
          ),
        ),
      );
    });
    mount(
      container,
      h(
        "header",
        { class: "view-header" },
        h("button", { class: "ghost", onclick: () => render() }, t("settings.perm.back")),
        h("h2", {}, t("settings.permissions.title")),
      ),
      card(
        t("settings.perm.capabilities"),
        h(
          "p",
          { class: "note" },
          t("settings.perm.capabilitiesNote"),
        ),
        ...rows,
        h(
          "button",
          {
            class: "primary",
            onclick: async () => {
              if (!(await attempt(() => api.savePermissions(permissions), t("guard.permissions")))) return;
              await ctx.refreshStatus();
              render();
              toast(t("settings.perm.saved"));
            },
          },
          t("settings.save"),
        ),
      ),
      card(
        t("settings.perm.pcTitle"),
        h(
          "p",
          { class: "note" },
          t("settings.perm.pcNote"),
        ),
        field(t("settings.perm.powerLabel"), powerSelect),
        h(
          "p",
          { class: "note" },
          t("settings.perm.webcamNote"),
        ),
        field(t("settings.perm.webcamLabel"), webcamSelect),
      ),
      card(
        t("settings.perm.alwaysTitle"),
        h(
          "p",
          { class: "note" },
          t("settings.perm.alwaysNote"),
        ),
        alwaysList,
      ),
    );
  }

  void load();
  return container;
}

/** Durées proposées pour la conversation continue : [secondes, clé i18n]. */
export const FOLLOW_UP_CHOICES: [number, Key][] = [
  [0, "voice.followUp.0"],
  [5, "voice.followUp.5"],
  [8, "voice.followUp.8"],
  [12, "voice.followUp.12"],
  [20, "voice.followUp.20"],
];

/** Tolérance aux hésitations (silence qui termine la phrase) : [ms, clé i18n]. */
export const HESITATION_CHOICES: [number, Key][] = [
  [700, "voice.hesitation.700"],
  [1200, "voice.hesitation.1200"],
  [1600, "voice.hesitation.1600"],
];

/** Longueur lue à voix haute : [caractères, clé i18n]. */
export const SPOKEN_LENGTH_CHOICES: [number, Key][] = [
  [240, "voice.spoken.240"],
  [900, "voice.spoken.900"],
  [6000, "voice.spoken.6000"],
];

/** Langues proposées pour la reconnaissance vocale (codes Whisper). */
export const LANGUAGES: [string, Key][] = [
  ["fr", "voice.lang.fr"],
  ["en", "voice.lang.en"],
  ["es", "voice.lang.es"],
  ["de", "voice.lang.de"],
  ["it", "voice.lang.it"],
  ["auto", "voice.lang.auto"],
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

/**
 * Langue de l'interface (version anglaise, 21 octobre). Enregistrée à part,
 * sur une copie fraîche des réglages, puis la fenêtre se recharge : aucun
 * formulaire resté ouvert ne peut l'écraser avec une copie ancienne.
 */
function languageCard(): HTMLElement {
  const select = h("select", { class: "field", "aria-label": t("lang.title") }) as HTMLSelectElement;
  for (const value of ["auto", "fr", "en"] as const) {
    select.append(h("option", { value }, t(`lang.${value}`)));
  }
  void api.getSettings().then((current) => (select.value = current.ui.language || "auto")).catch(() => undefined);
  select.addEventListener("change", async () => {
    const fresh = await guard(() => api.getSettings(), t("lang.title"));
    if (!fresh) return;
    fresh.ui.language = select.value;
    if (!(await attempt(() => api.saveSettings(fresh), t("lang.title")))) return;
    setLanguage(select.value);
    toast(t("lang.saved"));
    window.location.reload();
  });
  return card(t("lang.title"), h("p", { class: "note" }, t("lang.hint")), select);
}
