/**
 * Bibliothèque de modèles de langage, par fournisseur.
 *
 * La liste vient du compte du fournisseur sélectionné (ce qu'il accepte
 * réellement), enrichie par le catalogue public pour OpenCode Go. Chaque
 * modèle se **teste** dans les conditions de Jimy — une requête simple, puis
 * avec un outil — et un modèle qui échoue ne peut pas être choisi :
 * « fonctionnel » veut dire vérifié.
 *
 * Les résultats de test sont gardés dans le navigateur, par fournisseur et par
 * machine (`fournisseur:modèle`) : la latence varie selon la charge, un test
 * est une indication datée, pas une garantie.
 */
import { api, type ModelInfo, type ModelTest } from "../api";
import { attempt, h, mount, toast } from "../ui";
import type { AppContext } from "../context";
import { locale, t } from "../i18n";

type Role = "main" | "voice";
type Stored = ModelTest & { at: number };
type Sort = "recommended" | "name" | "speed" | "context" | "recent";

// v3 (8 octobre) : multi-fournisseurs — les clés de test deviennent
// `fournisseur:modèle`, un même nom de modèle n'a rien à voir d'un
// fournisseur à l'autre. v2 : les trois formats d'API (6 octobre).
const STORAGE_KEY = "jimmy.llm-tests.v3";
/** Un test plus vieux que cela est refait avant de choisir le modèle. */
const FRESH_MS = 24 * 3600 * 1000;
const CONCURRENCY = 3;

function loadTests(): Record<string, Stored> {
  try {
    return JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "{}") as Record<string, Stored>;
  } catch {
    return {};
  }
}

function saveTests(tests: Record<string, Stored>) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(tests));
  } catch {
    /* stockage indisponible : les résultats restent en mémoire pour cette vue */
  }
}

function formatContext(tokens: number): string {
  if (!tokens) return "—";
  if (tokens >= 1_000_000) return `${(tokens / 1_000_000).toFixed(tokens % 1_000_000 === 0 ? 0 : 1)} M`;
  return `${Math.round(tokens / 1000)} k`;
}

function formatCost(model: ModelInfo): string {
  if (model.free) return t("models.free");
  if (!model.in_catalog) return "?";
  const fmt = (n: number) => (n < 1 ? n.toFixed(2) : n.toFixed(1)).replace(".", ",");
  return `${fmt(model.cost_input)} / ${fmt(model.cost_output)} $`;
}

function formatSeconds(ms: number): string {
  return `${(ms / 1000).toFixed(1).replace(".", ",")} s`;
}

function ago(at: number): string {
  const minutes = Math.round((Date.now() - at) / 60000);
  if (minutes < 1) return t("models.ago.now");
  if (minutes < 60) return t("models.ago.min", { n: minutes });
  const hours = Math.round(minutes / 60);
  return hours < 48 ? t("models.ago.hours", { n: hours }) : t("models.ago.days", { n: Math.round(hours / 24) });
}

export interface ModelsPanelHooks {
  /** Appelé quand un modèle est appliqué : la vue Paramètres met ses champs à jour. */
  onApplied: (role: Role, model: string, provider?: string) => void;
}

export function modelsPanel(ctx: AppContext, hooks: ModelsPanelHooks, onlyRole?: Role): HTMLElement {
  let models: ModelInfo[] = [];
  const tests = loadTests();
  const testing = new Set<string>();
  let sort: Sort = "recommended";
  let query = "";
  let onlyWorking = false;
  let batchCancelled = false;
  let batchRunning = false;

  // Fournisseur affiché : celui du modèle principal au départ.
  let providerId = ctx.status.llm.provider || "opencode";

  const current = h("div", { class: "model-current" });
  const list = h("div", { class: "model-list", "aria-live": "polite" });
  const progress = h("span", { class: "note" }, "");
  const rows = new Map<string, HTMLElement>();

  const providerSelect = h("select", { class: "field", "aria-label": t("models.providerLabel") }) as HTMLSelectElement;
  const searchInput = h("input", {
    class: "field",
    type: "search",
    placeholder: t("models.searchPlaceholder"),
    "aria-label": t("models.searchLabel"),
  }) as HTMLInputElement;
  const sortSelect = h("select", { class: "field", "aria-label": t("models.sortLabel") }) as HTMLSelectElement;
  for (const [value, label] of [
    ["recommended", t("models.sort.recommended")],
    ["name", t("models.sort.name")],
    ["speed", t("models.sort.speed")],
    ["context", t("models.sort.context")],
    ["recent", t("models.sort.recent")],
  ] as const) {
    sortSelect.append(h("option", { value }, label));
  }
  const workingBox = h("input", { type: "checkbox" }) as HTMLInputElement;
  const refreshButton = h("button", { class: "ghost", onclick: () => void load(true) }, t("models.refresh"));
  const batchButton = h("button", { class: "primary", onclick: () => void toggleBatch() }, t("models.testAll"));

  // ── Données ────────────────────────────────────────────────────────────────

  async function load(refresh = false) {
    refreshButton.setAttribute("disabled", "");
    mount(list, h("div", { class: "empty" }, h("p", {}, t("models.loading"))));
    try {
      models = await api.listModels(refresh, providerId);
    } catch (error) {
      models = [];
      mount(
        list,
        h(
          "div",
          { class: "empty" },
          h("p", {}, t("models.loadFailed")),
          h("p", { class: "hint" }, String(error)),
        ),
      );
      refreshButton.removeAttribute("disabled");
      return;
    }
    refreshButton.removeAttribute("disabled");
    renderCurrent();
    renderList();
  }

  /** Les fournisseurs activés, pour le sélecteur. */
  function enabledProviders() {
    return (ctx.status.llm.providers ?? []).filter((p) => p.enabled);
  }

  function renderProviderSelect() {
    mount(providerSelect);
    for (const provider of enabledProviders()) {
      providerSelect.append(h("option", { value: provider.id }, provider.label));
    }
    if (![...providerSelect.options].some((o) => o.value === providerId)) {
      providerId = providerSelect.options[0]?.value ?? "opencode";
    }
    providerSelect.value = providerId;
  }

  // ── Test d'un modèle ───────────────────────────────────────────────────────

  /** Clé des résultats : un « deepseek-chat » testé chez DeepSeek ne dit rien
   *  de son homonyme chez OpenRouter. */
  const testKey = (id: string) => `${providerId}:${id}`;

  async function runTest(id: string): Promise<Stored | null> {
    const key = testKey(id);
    if (testing.has(key)) return null;
    testing.add(key);
    updateRow(id);
    let result: Stored;
    try {
      const test = await api.llmTestModel(id, providerId);
      result = { ...test, at: Date.now() };
    } catch (error) {
      result = {
        model: id,
        ok: false,
        tools: false,
        latency_ms: 0,
        tools_latency_ms: 0,
        reply: "",
        error: String(error),
        tested_at: new Date().toISOString(),
        at: Date.now(),
      };
    }
    tests[key] = result;
    saveTests(tests);
    testing.delete(key);
    updateRow(id);
    return result;
  }

  async function toggleBatch() {
    if (batchRunning) {
      batchCancelled = true;
      batchButton.textContent = t("models.stopping");
      return;
    }
    batchRunning = true;
    batchCancelled = false;
    batchButton.textContent = t("models.stopTest");
    batchButton.className = "ghost";
    // Les modèles absents du catalogue passent aussi : le compte les autorise.
    const queue = visibleModels().map((m) => m.id);
    let done = 0;
    progress.textContent = `0 / ${queue.length}`;
    const worker = async () => {
      while (!batchCancelled) {
        const id = queue.shift();
        if (!id) return;
        await runTest(id);
        done += 1;
        progress.textContent = `${done} / ${done + queue.length}`;
      }
    };
    await Promise.all(Array.from({ length: CONCURRENCY }, worker));
    batchRunning = false;
    batchButton.textContent = t("models.testAll");
    batchButton.className = "primary";
    const working = models.filter((m) => tests[testKey(m.id)]?.ok && tests[testKey(m.id)]?.tools).length;
    progress.textContent = batchCancelled
      ? t("models.stoppedCount", { done })
      : t("models.doneCount", { working, total: models.length });
    // Re-tri à la fin seulement : les lignes ne sautent pas pendant le test.
    renderList();
  }

  // ── Choix du modèle ────────────────────────────────────────────────────────

  async function choose(role: Role, id: string) {
    const key = testKey(id);
    let result: Stored | undefined = tests[key];
    // Un modèle n'est appliqué que s'il a été vu fonctionner récemment.
    if (!result || Date.now() - result.at > FRESH_MS) {
      toast(t("models.testingBeforeApply", { id }));
      result = (await runTest(id)) ?? undefined;
    }
    if (!result || !result.ok) {
      toast(t("models.testFailedCancel", { id, error: result?.error ?? t("models.testImpossible") }), "error");
      return;
    }
    if (!result.tools) {
      const accepted = window.confirm(t("models.noToolsConfirm", { id }));
      if (!accepted) return;
    }
    if (!(await attempt(() => api.setLlmModel(role, id, providerId), t("guard.modelChoice")))) return;
    hooks.onApplied(role, id, providerId);
    await ctx.refreshStatus();
    renderCurrent();
    for (const model of models) updateRow(model.id);
    toast(role === "main" ? t("models.mainChosen", { id }) : t("models.voiceChosen", { id }));
  }

  async function clearVoice() {
    if (!(await attempt(() => api.setLlmModel("voice", ""), t("guard.voiceModel")))) return;
    hooks.onApplied("voice", "", "");
    await ctx.refreshStatus();
    renderCurrent();
    for (const model of models) updateRow(model.id);
    toast(t("models.voiceCleared"));
  }

  // ── Affichage ──────────────────────────────────────────────────────────────

  function providerLabel(id: string): string {
    return ctx.status.llm.providers?.find((p) => p.id === id)?.label ?? id;
  }

  function renderCurrent() {
    const main = ctx.status.llm.model;
    const voice = ctx.status.llm.voice_model ?? "";
    const name = (id: string) => models.find((m) => m.id === id)?.name;
    // Panneau restreint à un rôle (onglet Voix) : on ne montre que sa ligne.
    const showMain = onlyRole !== "voice";
    const showVoice = onlyRole !== "main";
    mount(
      current,
      showMain
        ? h(
            "div",
            {},
            h("span", { class: "model-role" }, t("models.role.main")),
            h("code", {}, main),
            name(main) ? ` · ${name(main)}` : "",
            ` — ${providerLabel(ctx.status.llm.provider)}`,
          )
        : null,
      showVoice
        ? h(
            "div",
            {},
            h("span", { class: "model-role" }, t("models.role.voice")),
            voice ? h("code", {}, voice) : h("em", {}, t("models.sameAsMain")),
            voice && name(voice) ? ` · ${name(voice)}` : "",
            voice ? ` — ${providerLabel(ctx.status.llm.voice_provider || ctx.status.llm.provider)}` : "",
            voice ? h("button", { class: "ghost small", onclick: () => void clearVoice() }, t("models.clearVoice")) : null,
          )
        : null,
    );
  }

  function status(model: ModelInfo): HTMLElement {
    const key = testKey(model.id);
    if (testing.has(key)) {
      return h("span", { class: "model-test testing" }, h("span", { class: "dots" }, h("i"), h("i"), h("i")), t("models.testing"));
    }
    const test = tests[key];
    if (!test) return h("span", { class: "model-test none", title: t("models.untestedHint") }, t("models.untested"));
    const when = ago(test.at);
    if (test.ok && test.tools) {
      return h(
        "span",
        {
          class: "model-test good",
          title: t("models.goodHint", { latency: test.latency_ms, toolsLatency: test.tools_latency_ms, when }),
        },
        `✓ ${formatSeconds(test.latency_ms)}`,
      );
    }
    if (test.ok) {
      return h("span", { class: "model-test warn", title: t("models.testedWhen", { error: test.error, when }) }, t("models.warnNoTools"));
    }
    return h("span", { class: "model-test bad", title: t("models.testedWhen", { error: test.error, when }) }, t("models.badFailed"));
  }

  function badge(label: string, title: string, kind = ""): HTMLElement {
    return h("span", { class: `badge ${kind}`, title }, label);
  }

  function buildRow(model: ModelInfo): HTMLElement {
    const isMain = model.id === ctx.status.llm.model && ctx.status.llm.provider === providerId;
    const isVoice = model.id === (ctx.status.llm.voice_model ?? "") && providerId === (ctx.status.llm.voice_provider || ctx.status.llm.provider);
    const test = tests[testKey(model.id)];
    const broken = Boolean(test && !test.ok);
    const busy = testing.has(testKey(model.id));
    const capabilities: HTMLElement[] = [];
    if (model.reasoning) capabilities.push(badge(t("models.cap.reasoning"), t("models.cap.reasoningHint")));
    if (model.vision) capabilities.push(badge(t("models.cap.vision"), t("models.cap.visionHint")));
    if (model.tool_call === false) capabilities.push(badge(t("models.cap.noTools"), t("models.cap.noToolsHint"), "warn"));
    if (!model.in_catalog) capabilities.push(badge(t("models.cap.uncataloged"), t("models.cap.uncatalogedHint")));

    return h(
      "div",
      { class: `model-row${isMain ? " is-main" : ""}${isVoice ? " is-voice" : ""}${broken ? " is-broken" : ""}` },
      h(
        "div",
        { class: "model-id" },
        h(
          "strong",
          {},
          model.name,
          isMain ? badge(t("models.badge.main"), t("models.badge.mainHint"), "main") : null,
          isVoice ? badge(t("models.badge.voice"), t("models.badge.voiceHint"), "voice") : null,
        ),
        h("code", {}, model.id),
        model.description ? h("p", { class: "model-desc", title: model.description }, model.description) : null,
      ),
      h(
        "div",
        { class: "model-meta" },
        h("span", { title: t("models.contextHint") }, t("models.context", { ctx: formatContext(model.context) })),
        h("span", { title: t("models.priceHint") }, formatCost(model)),
        h("div", { class: "badges" }, ...capabilities),
      ),
      status(model),
      h(
        "div",
        { class: "model-actions" },
        h(
          "button",
          { class: "ghost small", disabled: busy, onclick: () => void runTest(model.id), title: t("models.testButtonHint") },
          t("models.testButton"),
        ),
        onlyRole === "voice"
          ? null
          : h(
              "button",
              {
                class: isMain ? "primary small" : "ghost small",
                disabled: busy || isMain || broken,
                title: broken ? t("models.brokenHint") : t("models.useForMainHint"),
                onclick: () => void choose("main", model.id),
              },
              t("models.role.main"),
            ),
        onlyRole === "main"
          ? null
          : h(
              "button",
              {
                class: isVoice ? "primary small" : "ghost small",
                disabled: busy || isVoice || broken,
                title: broken ? t("models.brokenHint") : t("models.useForVoiceHint"),
                onclick: () => void choose("voice", model.id),
              },
              t("models.role.voice"),
            ),
      ),
    );
  }

  function updateRow(id: string) {
    const model = models.find((m) => m.id === id);
    const old = rows.get(id);
    if (!model || !old) return;
    const fresh = buildRow(model);
    old.replaceWith(fresh);
    rows.set(id, fresh);
  }

  function visibleModels(): ModelInfo[] {
    const needle = query.trim().toLowerCase();
    return sorted(
      models.filter((m) => {
        if (needle && !`${m.name} ${m.id} ${m.family} ${m.description}`.toLowerCase().includes(needle)) return false;
        if (onlyWorking) {
          const t = tests[testKey(m.id)];
          return Boolean(t?.ok && t.tools);
        }
        return true;
      }),
    );
  }

  function rank(m: ModelInfo): number {
    const isMainNow = m.id === ctx.status.llm.model && ctx.status.llm.provider === providerId;
    const isVoiceNow = m.id === (ctx.status.llm.voice_model ?? "") && providerId === (ctx.status.llm.voice_provider || ctx.status.llm.provider);
    if (isMainNow) return 0;
    if (isVoiceNow) return 1;
    const t = tests[testKey(m.id)];
    if (t?.ok && t.tools) return 2;
    if (!t) return 3;
    return t.ok ? 4 : 5;
  }

  function sorted(items: ModelInfo[]): ModelInfo[] {
    const byName = (a: ModelInfo, b: ModelInfo) => a.name.localeCompare(b.name, locale());
    const copy = [...items];
    switch (sort) {
      case "name":
        return copy.sort(byName);
      case "context":
        return copy.sort((a, b) => b.context - a.context || byName(a, b));
      case "recent":
        return copy.sort((a, b) => b.released.localeCompare(a.released) || byName(a, b));
      case "speed":
        return copy.sort((a, b) => {
          const ta = tests[testKey(a.id)]?.ok ? tests[testKey(a.id)].latency_ms : Infinity;
          const tb = tests[testKey(b.id)]?.ok ? tests[testKey(b.id)].latency_ms : Infinity;
          return ta - tb || byName(a, b);
        });
      default:
        return copy.sort((a, b) => {
          const delta = rank(a) - rank(b);
          if (delta) return delta;
          const ta = tests[testKey(a.id)]?.ok ? tests[testKey(a.id)].latency_ms : Infinity;
          const tb = tests[testKey(b.id)]?.ok ? tests[testKey(b.id)].latency_ms : Infinity;
          return ta - tb || byName(a, b);
        });
    }
  }

  function renderList() {
    rows.clear();
    const items = visibleModels();
    if (items.length === 0) {
      mount(
        list,
        h(
          "div",
          { class: "empty" },
          h("p", {}, models.length ? t("models.noMatch") : t("models.noneAvailable")),
          onlyWorking ? h("p", { class: "hint" }, t("models.onlyWorkingHint")) : null,
        ),
      );
      return;
    }
    mount(list);
    for (const model of items) {
      const row = buildRow(model);
      rows.set(model.id, row);
      list.append(row);
    }
  }

  providerSelect.addEventListener("change", () => {
    providerId = providerSelect.value;
    void load();
  });
  searchInput.addEventListener("input", () => {
    query = searchInput.value;
    renderList();
  });
  sortSelect.addEventListener("change", () => {
    sort = sortSelect.value as Sort;
    renderList();
  });
  workingBox.addEventListener("change", () => {
    onlyWorking = workingBox.checked;
    renderList();
  });
  // La carte Fournisseurs a ajouté, retiré ou activé un fournisseur : le
  // sélecteur doit suivre (écouteur retiré à la sortie de la vue).
  const onProvidersChanged = () => renderProviderSelect();
  window.addEventListener("jimmy-providers-changed", onProvidersChanged);
  ctx.onCleanup(() => window.removeEventListener("jimmy-providers-changed", onProvidersChanged));

  renderProviderSelect();
  renderCurrent();
  void load();

  return h(
    "section",
    { class: "card models-panel" },
    h("h3", {}, onlyRole === "voice" ? t("models.titleVoice") : t("models.titleMain")),
    h(
      "p",
      { class: "note" },
      onlyRole === "voice" ? t("models.noteVoice") : t("models.noteMain"),
    ),
    current,
    h(
      "div",
      { class: "model-toolbar" },
      providerSelect,
      searchInput,
      sortSelect,
      h("label", { class: "toggle" }, workingBox, h("span", {}, t("models.onlyWorking"))),
    ),
    h("div", { class: "row" }, batchButton, refreshButton, progress),
    list,
  );
}
