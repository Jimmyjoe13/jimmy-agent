/**
 * Fournisseurs de modèles de langage (Paramètres → LLM).
 *
 * Chaque fournisseur est une ligne : URL de base, format d'API, activé, clé.
 * Les clés ne remontent JAMAIS de Rust (`get_settings` les masque) : l'état
 * vient de `status` (has_key, key_from, key_hint) et la saisie passe par la
 * commande dédiée `llm_set_provider_key`. La clé de l'environnement (.env)
 * reste un repli pour les fournisseurs intégrés.
 *
 * Classes propres à cette vue (`provider-row`, `provider-key`) : les réutiliser
 * depuis une autre fausse ses tests (piège 63).
 */
import { api, type ProviderConfig, type ProviderStatus } from "../api";
import { attempt, guard, h, mount, toast } from "../ui";
import type { AppContext } from "../context";
import { t, type Key } from "../i18n";

/** Carte locale : même style que `card` de settings.ts, sans import circulaire. */
function card(title: string, ...children: (Node | string | null | false)[]): HTMLElement {
  return h("section", { class: "card" }, h("h3", {}, title), ...children);
}

export interface ProvidersHooks {
  /** Après tout changement : la vue Paramètres recharge sa copie, le panneau
   *  des modèles rafraîchit sa liste de fournisseurs. */
  onChanged: () => void;
}

const PROTOCOLS: [string, Key][] = [
  ["auto", "providers.proto.auto"],
  ["chat", "providers.proto.chat"],
  ["responses", "providers.proto.responses"],
  ["messages", "providers.proto.messages"],
  ["google", "providers.proto.google"],
];

export function providersPanel(ctx: AppContext, hooks: ProvidersHooks): HTMLElement {
  const container = h("div", { class: "providers-panel" });
  let configs: ProviderConfig[] = [];

  const addLabel = h("input", { class: "field", type: "text", placeholder: t("providers.addLabelPlaceholder") }) as HTMLInputElement;
  const addUrl = h("input", { class: "field", type: "text", placeholder: "http://127.0.0.1:1234/v1" }) as HTMLInputElement;
  const addProtocol = h("select", { class: "field" }) as HTMLSelectElement;
  const addKey = h("input", { class: "field provider-key", type: "password", placeholder: t("providers.addKeyPlaceholder") }) as HTMLInputElement;
  for (const [value, labelKey] of PROTOCOLS) addProtocol.append(h("option", { value }, t(labelKey)));

  function statusFor(id: string): ProviderStatus | undefined {
    return ctx.status.llm.providers?.find((p) => p.id === id);
  }

  async function reload() {
    const loaded = await guard(() => api.getSettings(), t("guard.providers"));
    if (!loaded) return;
    configs = loaded.llm.providers ?? [];
    await ctx.refreshStatus();
    render();
    // Le panneau des modèles tient sa liste de fournisseurs du status : il la
    // reconstruit après tout changement ici (ajout, suppression, activation).
    window.dispatchEvent(new Event("jimmy-providers-changed"));
  }

  function keyState(status: ProviderStatus | undefined): HTMLElement {
    if (!status) return h("span", { class: "badge" }, t("providers.state.unknown"));
    if (status.key_from === "abonnement") {
      return h("span", { class: "badge ok", title: t("providers.state.subHint") }, t("providers.state.sub", { sub: status.subscription ? ` ${status.subscription}` : "" }));
    }
    if (status.key_from === "interface") {
      return h("span", { class: "badge ok", title: t("providers.state.uiHint") }, t("providers.state.ui", { hint: status.key_hint ?? "" }));
    }
    if (status.key_from === "environnement") {
      return h("span", { class: "badge ok", title: t("providers.state.envHint") }, t("providers.state.env"));
    }
    if (status.auth === "claude-plan") {
      return h("span", { class: "badge warn", title: t("providers.state.noSessionHint") }, t("providers.state.noSession"));
    }
    return h("span", { class: "badge warn", title: t("providers.state.noKeyHint") }, t("providers.state.noKey"));
  }

  function row(config: ProviderConfig): HTMLElement {
    const status = statusFor(config.id);
    const urlInput = h("input", { class: "field", type: "text", value: config.base_url }) as HTMLInputElement;
    const protocolSelect = h("select", { class: "field provider-protocol" }) as HTMLSelectElement;
    for (const [value, labelKey] of PROTOCOLS) protocolSelect.append(h("option", { value }, t(labelKey)));
    protocolSelect.value = config.protocol ?? "auto";
    const enabledBox = h("input", { type: "checkbox" }) as HTMLInputElement;
    enabledBox.checked = config.enabled;
    // Seul Anthropic connaît l'abonnement Claude (session Claude Code).
    const authSelect =
      config.id === "anthropic"
        ? (h("select", { class: "field provider-auth", title: t("providers.authHint") }) as HTMLSelectElement)
        : null;
    if (authSelect) {
      authSelect.append(
        h("option", { value: "api-key" }, t("providers.auth.apiKey")),
        h("option", { value: "claude-plan" }, t("providers.auth.claudePlan")),
      );
      authSelect.value = config.auth || "api-key";
    }
    const planMode = () => authSelect?.value === "claude-plan";
    const keyInput = h("input", {
      class: "field provider-key",
      type: "password",
      placeholder:
        status?.key_from === "abonnement"
          ? t("providers.keyPlaceholder.sub")
          : status?.key_from === "interface"
            ? t("providers.keyPlaceholder.ui", { hint: status.key_hint ?? "" })
            : t("providers.keyPlaceholder.empty"),
    }) as HTMLInputElement;

    async function save() {
      // Abonnement Claude : aucun secret à écrire, la clé est ignorée.
      const key = keyInput.value.trim();
      if (!planMode() && key && !(await attempt(() => api.llmSetProviderKey(config.id, key), t("guard.key")))) return;
      if (
        !(await attempt(
          () =>
            api.llmUpdateProvider({
              provider: config.id,
              label: config.label,
              baseUrl: urlInput.value.trim(),
              protocol: protocolSelect.value === "auto" ? null : protocolSelect.value,
              auth: authSelect?.value ?? null,
              enabled: enabledBox.checked,
            }),
          t("guard.provider"),
        ))
      )
        return;
      keyInput.value = "";
      toast(planMode() ? t("providers.savedPlan", { label: config.label }) : t("providers.saved", { label: config.label }));
      hooks.onChanged();
      await reload();
    }

    const buttons: HTMLElement[] = [
      h("button", { class: "primary small", onclick: () => void save() }, t("providers.save")),
      h(
        "button",
        {
          class: "ghost small",
          title: t("providers.testHint"),
          onclick: async () => {
            const count = await guard(() => api.llmCheckProvider(config.id), t("guard.connection"));
            if (count !== undefined) toast(t("providers.testCount", { count, label: config.label }));
          },
        },
        t("providers.test"),
      ),
    ];
    if (status?.key_from === "interface") {
      buttons.push(
        h(
          "button",
          {
            class: "ghost small",
            title: t("providers.removeKeyHint"),
            onclick: async () => {
              if (!(await attempt(() => api.llmSetProviderKey(config.id, ""), t("guard.removeKey")))) return;
              toast(t("providers.keyRemoved"));
              hooks.onChanged();
              await reload();
            },
          },
          t("providers.removeKey"),
        ),
      );
    }
    if (!config.builtin) {
      buttons.push(
        h(
          "button",
          {
            class: "ghost small",
            onclick: async () => {
              if (!(await attempt(() => api.llmRemoveProvider(config.id), t("guard.delete")))) return;
              toast(t("providers.removed", { label: config.label }));
              hooks.onChanged();
              await reload();
            },
          },
          t("providers.delete"),
        ),
      );
    }

    const badges: HTMLElement[] = [];
    if (ctx.status.llm.provider === config.id) badges.push(h("span", { class: "badge main" }, t("providers.badge.main")));
    const voiceProvider = ctx.status.llm.voice_provider || ctx.status.llm.provider;
    if (voiceProvider === config.id && ctx.status.llm.voice_model) badges.push(h("span", { class: "badge voice" }, t("providers.badge.voice")));
    if (config.builtin) badges.push(h("span", { class: "badge", title: t("providers.badge.builtinHint") }, t("providers.badge.builtin")));

    return h(
      "div",
      { class: `provider-row${config.id === ctx.status.llm.provider ? " is-main" : ""}` },
      h(
        "div",
        { class: "provider-id" },
        h("strong", {}, config.label, ...badges),
        h("code", {}, config.id),
        keyState(status),
      ),
      h("div", { class: "provider-fields" }, urlInput, protocolSelect, ...(authSelect ? [authSelect] : []), h("label", { class: "toggle" }, enabledBox, h("span", {}, t("providers.enabled"))), keyInput),
      h("div", { class: "provider-actions" }, ...buttons),
    );
  }

  async function add() {
    const label = addLabel.value.trim();
    const baseUrl = addUrl.value.trim();
    if (!label || !baseUrl) {
      toast(t("providers.addRequired"), "error");
      return;
    }
    const id = await guard(
      () => api.llmAddProvider({ label, baseUrl, protocol: addProtocol.value === "auto" ? null : addProtocol.value, apiKey: addKey.value.trim() || undefined }),
      t("guard.addProvider"),
    );
    if (!id) return;
    addLabel.value = "";
    addUrl.value = "";
    addKey.value = "";
    toast(t("providers.added", { label }));
    hooks.onChanged();
    await reload();
  }

  function render() {
    mount(
      container,
      card(
        t("providers.title"),
        h(
          "p",
          { class: "note" },
          t("providers.note1"),
        ),
        h(
          "p",
          { class: "note" },
          t("providers.note2"),
        ),
        ...configs.map(row),
        h(
          "div",
          { class: "provider-row provider-add" },
          h("strong", {}, t("providers.addTitle")),
          h("div", { class: "provider-fields" }, addLabel, addUrl, addProtocol, addKey),
          h("div", { class: "provider-actions" }, h("button", { class: "ghost small", onclick: () => void add() }, t("providers.addButton"))),
        ),
      ),
    );
  }

  void reload();
  return container;
}
