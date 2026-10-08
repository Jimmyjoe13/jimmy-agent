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

/** Carte locale : même style que `card` de settings.ts, sans import circulaire. */
function card(title: string, ...children: (Node | string | null | false)[]): HTMLElement {
  return h("section", { class: "card" }, h("h3", {}, title), ...children);
}

export interface ProvidersHooks {
  /** Après tout changement : la vue Paramètres recharge sa copie, le panneau
   *  des modèles rafraîchit sa liste de fournisseurs. */
  onChanged: () => void;
}

const PROTOCOLS: [string, string][] = [
  ["auto", "auto (catalogue, puis essais)"],
  ["chat", "chat — /chat/completions"],
  ["responses", "responses — /responses"],
  ["messages", "messages — /messages (Claude)"],
];

export function providersPanel(ctx: AppContext, hooks: ProvidersHooks): HTMLElement {
  const container = h("div", { class: "providers-panel" });
  let configs: ProviderConfig[] = [];

  const addLabel = h("input", { class: "field", type: "text", placeholder: "Nom — ex. LM Studio" }) as HTMLInputElement;
  const addUrl = h("input", { class: "field", type: "text", placeholder: "http://127.0.0.1:1234/v1" }) as HTMLInputElement;
  const addProtocol = h("select", { class: "field" }) as HTMLSelectElement;
  const addKey = h("input", { class: "field provider-key", type: "password", placeholder: "clé (facultative)" }) as HTMLInputElement;
  for (const [value, label] of PROTOCOLS) addProtocol.append(h("option", { value }, label));

  function statusFor(id: string): ProviderStatus | undefined {
    return ctx.status.llm.providers?.find((p) => p.id === id);
  }

  async function reload() {
    const loaded = await guard(() => api.getSettings(), "fournisseurs");
    if (!loaded) return;
    configs = loaded.llm.providers ?? [];
    await ctx.refreshStatus();
    render();
    // Le panneau des modèles tient sa liste de fournisseurs du status : il la
    // reconstruit après tout changement ici (ajout, suppression, activation).
    window.dispatchEvent(new Event("jimmy-providers-changed"));
  }

  function keyState(status: ProviderStatus | undefined): HTMLElement {
    if (!status) return h("span", { class: "badge" }, "état inconnu");
    if (status.key_from === "abonnement") {
      return h("span", { class: "badge ok", title: "Jeton lu dans la session Claude Code (~/.claude/.credentials.json), rafraîchi par Jimmy" }, `abonnement${status.subscription ? ` ${status.subscription}` : ""}`);
    }
    if (status.key_from === "interface") {
      return h("span", { class: "badge ok", title: "Clé saisie dans l'interface, stockée dans config.json (jamais journalisée)" }, `clé ••••${status.key_hint ?? ""}`);
    }
    if (status.key_from === "environnement") {
      return h("span", { class: "badge ok", title: "Clé lue dans le fichier .env : repli seulement, une saisie ici gagne dessus" }, "clé .env");
    }
    if (status.auth === "claude-plan") {
      return h("span", { class: "badge warn", title: "Ouvre Claude Code (claude /login) pour rétablir la session" }, "session Claude Code introuvable");
    }
    return h("span", { class: "badge warn", title: "Aucune clé : saisis-la, ou une variable d'environnement pour les intégrés" }, "aucune clé");
  }

  function row(config: ProviderConfig): HTMLElement {
    const status = statusFor(config.id);
    const urlInput = h("input", { class: "field", type: "text", value: config.base_url }) as HTMLInputElement;
    const protocolSelect = h("select", { class: "field provider-protocol" }) as HTMLSelectElement;
    for (const [value, label] of PROTOCOLS) protocolSelect.append(h("option", { value }, label));
    protocolSelect.value = config.protocol ?? "auto";
    const enabledBox = h("input", { type: "checkbox" }) as HTMLInputElement;
    enabledBox.checked = config.enabled;
    // Seul Anthropic connaît l'abonnement Claude (session Claude Code).
    const authSelect =
      config.id === "anthropic"
        ? (h("select", { class: "field provider-auth", title: "Méthode d'accès" }) as HTMLSelectElement)
        : null;
    if (authSelect) {
      authSelect.append(
        h("option", { value: "api-key" }, "Clé API"),
        h("option", { value: "claude-plan" }, "Abonnement Claude (session Claude Code)"),
      );
      authSelect.value = config.auth || "api-key";
    }
    const planMode = () => authSelect?.value === "claude-plan";
    const keyInput = h("input", {
      class: "field provider-key",
      type: "password",
      placeholder:
        status?.key_from === "abonnement"
          ? "abonnement actif : aucun secret à saisir"
          : status?.key_from === "interface"
            ? `••••${status.key_hint ?? ""} (inchangée si vide)`
            : "sk-… (la clé ne part jamais du Rust)",
    }) as HTMLInputElement;

    async function save() {
      // Abonnement Claude : aucun secret à écrire, la clé est ignorée.
      const key = keyInput.value.trim();
      if (!planMode() && key && !(await attempt(() => api.llmSetProviderKey(config.id, key), "clé"))) return;
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
          "fournisseur",
        ))
      )
        return;
      keyInput.value = "";
      toast(planMode() ? `« ${config.label} » — la session Claude Code est utilisée` : `« ${config.label} » enregistré`);
      hooks.onChanged();
      await reload();
    }

    const buttons: HTMLElement[] = [
      h("button", { class: "primary small", onclick: () => void save() }, "Enregistrer"),
      h(
        "button",
        {
          class: "ghost small",
          title: "Liste les modèles accessibles : prouve que la clé et l'URL sont bonnes",
          onclick: async () => {
            const count = await guard(() => api.llmCheckProvider(config.id), "connexion");
            if (count !== undefined) toast(`${count} modèle(s) accessible(s) chez « ${config.label} »`);
          },
        },
        "Tester",
      ),
    ];
    if (status?.key_from === "interface") {
      buttons.push(
        h(
          "button",
          {
            class: "ghost small",
            title: "Retirer la clé saisie (le repli .env reste disponible)",
            onclick: async () => {
              if (!(await attempt(() => api.llmSetProviderKey(config.id, ""), "retrait de clé"))) return;
              toast("Clé retirée");
              hooks.onChanged();
              await reload();
            },
          },
          "Retirer la clé",
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
              if (!(await attempt(() => api.llmRemoveProvider(config.id), "suppression"))) return;
              toast(`« ${config.label} » retiré`);
              hooks.onChanged();
              await reload();
            },
          },
          "Supprimer",
        ),
      );
    }

    const badges: HTMLElement[] = [];
    if (ctx.status.llm.provider === config.id) badges.push(h("span", { class: "badge main" }, "principal"));
    const voiceProvider = ctx.status.llm.voice_provider || ctx.status.llm.provider;
    if (voiceProvider === config.id && ctx.status.llm.voice_model) badges.push(h("span", { class: "badge voice" }, "vocal"));
    if (config.builtin) badges.push(h("span", { class: "badge", title: "Fournisseur proposé d'office : désactivable, pas supprimable" }, "intégré"));

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
      h("div", { class: "provider-fields" }, urlInput, protocolSelect, ...(authSelect ? [authSelect] : []), h("label", { class: "toggle" }, enabledBox, h("span", {}, "activé")), keyInput),
      h("div", { class: "provider-actions" }, ...buttons),
    );
  }

  async function add() {
    const label = addLabel.value.trim();
    const baseUrl = addUrl.value.trim();
    if (!label || !baseUrl) {
      toast("Nom et URL de base sont requis.", "error");
      return;
    }
    const id = await guard(
      () => api.llmAddProvider({ label, baseUrl, protocol: addProtocol.value === "auto" ? null : addProtocol.value, apiKey: addKey.value.trim() || undefined }),
      "ajout de fournisseur",
    );
    if (!id) return;
    addLabel.value = "";
    addUrl.value = "";
    addKey.value = "";
    toast(`« ${label} » ajouté`);
    hooks.onChanged();
    await reload();
  }

  function render() {
    mount(
      container,
      card(
        "Fournisseurs",
        h(
          "p",
          { class: "note" },
          "Clé et URL par fournisseur. Le modèle principal se choisit dans la bibliothèque, sous cette carte ; le modèle vocal dans l'onglet Voix. « Tester » liste les modèles accessibles (une requête, comme les tiennes).",
        ),
        h(
          "p",
          { class: "note" },
          "Les clés sont stockées dans data/config.json (jamais commité, jamais journalisé) ; une clé vide laisse la ligne inchangée. Pour OpenCode Go et OpenRouter, la variable d'environnement sert de repli si rien n'est saisi ici.",
        ),
        ...configs.map(row),
        h(
          "div",
          { class: "provider-row provider-add" },
          h("strong", {}, "Ajouter un fournisseur"),
          h("div", { class: "provider-fields" }, addLabel, addUrl, addProtocol, addKey),
          h("div", { class: "provider-actions" }, h("button", { class: "ghost small", onclick: () => void add() }, "Ajouter")),
        ),
      ),
    );
  }

  void reload();
  return container;
}
