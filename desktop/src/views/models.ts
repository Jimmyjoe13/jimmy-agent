/**
 * Bibliothèque de modèles de langage (OpenCode Go).
 *
 * La liste vient du compte (ce que le fournisseur accepte réellement),
 * enrichie par le catalogue public. Chaque modèle se **teste** dans les
 * conditions de Jimmy — une requête simple, puis avec un outil — et un modèle
 * qui échoue ne peut pas être choisi : « fonctionnel » veut dire vérifié.
 *
 * Les résultats de test sont gardés dans le navigateur (par machine) : la
 * latence du fournisseur varie de 2 s à 25 s selon la charge, un test est une
 * indication datée, pas une garantie.
 */
import { api, type ModelInfo, type ModelTest } from "../api";
import { attempt, h, mount, toast } from "../ui";
import type { AppContext } from "../context";

type Role = "main" | "voice";
type Stored = ModelTest & { at: number };
type Sort = "recommended" | "name" | "speed" | "context" | "recent";

const STORAGE_KEY = "jimmy.llm-tests.v1";
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
  if (model.free) return "gratuit";
  if (!model.in_catalog) return "?";
  const fmt = (n: number) => (n < 1 ? n.toFixed(2) : n.toFixed(1)).replace(".", ",");
  return `${fmt(model.cost_input)} / ${fmt(model.cost_output)} $`;
}

function formatSeconds(ms: number): string {
  return `${(ms / 1000).toFixed(1).replace(".", ",")} s`;
}

function ago(at: number): string {
  const minutes = Math.round((Date.now() - at) / 60000);
  if (minutes < 1) return "à l'instant";
  if (minutes < 60) return `il y a ${minutes} min`;
  const hours = Math.round(minutes / 60);
  return hours < 48 ? `il y a ${hours} h` : `il y a ${Math.round(hours / 24)} j`;
}

export interface ModelsPanelHooks {
  /** Appelé quand un modèle est appliqué : la vue Paramètres met ses champs à jour. */
  onApplied: (role: Role, model: string) => void;
}

export function modelsPanel(ctx: AppContext, hooks: ModelsPanelHooks): HTMLElement {
  let models: ModelInfo[] = [];
  const tests = loadTests();
  const testing = new Set<string>();
  let sort: Sort = "recommended";
  let query = "";
  let onlyWorking = false;
  let batchCancelled = false;
  let batchRunning = false;

  let mainModel = ctx.status.llm.model;
  let voiceModel = ctx.status.llm.voice_model ?? "";

  const current = h("div", { class: "model-current" });
  const list = h("div", { class: "model-list", "aria-live": "polite" });
  const progress = h("span", { class: "note" }, "");
  const rows = new Map<string, HTMLElement>();

  const searchInput = h("input", {
    class: "field",
    type: "search",
    placeholder: "Rechercher un modèle…",
    "aria-label": "Rechercher un modèle",
  }) as HTMLInputElement;
  const sortSelect = h("select", { class: "field", "aria-label": "Tri" }) as HTMLSelectElement;
  for (const [value, label] of [
    ["recommended", "Tri : recommandés"],
    ["name", "Tri : nom"],
    ["speed", "Tri : vitesse (testés)"],
    ["context", "Tri : contexte"],
    ["recent", "Tri : récents"],
  ] as const) {
    sortSelect.append(h("option", { value }, label));
  }
  const workingBox = h("input", { type: "checkbox" }) as HTMLInputElement;
  const refreshButton = h("button", { class: "ghost", onclick: () => void load(true) }, "Actualiser la liste");
  const batchButton = h("button", { class: "primary", onclick: () => void toggleBatch() }, "Tester tous les modèles");

  // ── Données ────────────────────────────────────────────────────────────────

  async function load(refresh = false) {
    refreshButton.setAttribute("disabled", "");
    mount(list, h("div", { class: "empty" }, h("p", {}, "Chargement des modèles du compte…")));
    try {
      models = await api.listModels(refresh);
    } catch (error) {
      models = [];
      mount(
        list,
        h(
          "div",
          { class: "empty" },
          h("p", {}, "Impossible de récupérer la liste des modèles."),
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

  // ── Test d'un modèle ───────────────────────────────────────────────────────

  async function runTest(id: string): Promise<Stored | null> {
    if (testing.has(id)) return null;
    testing.add(id);
    updateRow(id);
    let result: Stored;
    try {
      const test = await api.llmTestModel(id);
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
    tests[id] = result;
    saveTests(tests);
    testing.delete(id);
    updateRow(id);
    return result;
  }

  async function toggleBatch() {
    if (batchRunning) {
      batchCancelled = true;
      batchButton.textContent = "Arrêt…";
      return;
    }
    batchRunning = true;
    batchCancelled = false;
    batchButton.textContent = "Arrêter le test";
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
    batchButton.textContent = "Tester tous les modèles";
    batchButton.className = "primary";
    const working = models.filter((m) => tests[m.id]?.ok && tests[m.id]?.tools).length;
    progress.textContent = batchCancelled
      ? `Arrêté — ${done} testé(s)`
      : `Terminé : ${working} modèle(s) fonctionnel(s) sur ${models.length}`;
    // Re-tri à la fin seulement : les lignes ne sautent pas pendant le test.
    renderList();
  }

  // ── Choix du modèle ────────────────────────────────────────────────────────

  async function choose(role: Role, id: string) {
    let result: Stored | undefined = tests[id];
    // Un modèle n'est appliqué que s'il a été vu fonctionner récemment.
    if (!result || Date.now() - result.at > FRESH_MS) {
      toast(`Test de « ${id} » avant de l'appliquer…`);
      result = (await runTest(id)) ?? undefined;
    }
    if (!result || !result.ok) {
      toast(`« ${id} » ne répond pas : ${result?.error ?? "test impossible"}. Choix annulé.`, "error");
      return;
    }
    if (!result.tools) {
      const accepted = window.confirm(
        `« ${id} » répond, mais refuse les outils.\n\nAvec lui, Jimmy ne pourrait plus lire de fichiers ni lancer de commandes : il ne ferait que discuter.\n\nL'appliquer quand même ?`,
      );
      if (!accepted) return;
    }
    if (!(await attempt(() => api.setLlmModel(role, id), "choix du modèle"))) return;
    if (role === "main") mainModel = id;
    else voiceModel = id;
    hooks.onApplied(role, id);
    await ctx.refreshStatus();
    renderCurrent();
    for (const model of models) updateRow(model.id);
    toast(role === "main" ? `Modèle principal : ${id}` : `Modèle vocal : ${id}`);
  }

  async function clearVoice() {
    if (!(await attempt(() => api.setLlmModel("voice", ""), "modèle vocal"))) return;
    voiceModel = "";
    hooks.onApplied("voice", "");
    await ctx.refreshStatus();
    renderCurrent();
    for (const model of models) updateRow(model.id);
    toast("Le modèle vocal est le même que le principal.");
  }

  // ── Affichage ──────────────────────────────────────────────────────────────

  function renderCurrent() {
    const name = (id: string) => models.find((m) => m.id === id)?.name;
    mount(
      current,
      h(
        "div",
        {},
        h("span", { class: "model-role" }, "Principal"),
        h("code", {}, mainModel),
        name(mainModel) ? ` · ${name(mainModel)}` : "",
      ),
      h(
        "div",
        {},
        h("span", { class: "model-role" }, "Vocal"),
        voiceModel ? h("code", {}, voiceModel) : h("em", {}, "même modèle que le principal"),
        voiceModel && name(voiceModel) ? ` · ${name(voiceModel)}` : "",
        voiceModel ? h("button", { class: "ghost small", onclick: () => void clearVoice() }, "Retirer") : null,
      ),
    );
  }

  function status(model: ModelInfo): HTMLElement {
    if (testing.has(model.id)) {
      return h("span", { class: "model-test testing" }, h("span", { class: "dots" }, h("i"), h("i"), h("i")), " test");
    }
    const test = tests[model.id];
    if (!test) return h("span", { class: "model-test none", title: "Pas encore testé" }, "non testé");
    const when = ago(test.at);
    if (test.ok && test.tools) {
      return h(
        "span",
        {
          class: "model-test good",
          title: `Répond en ${test.latency_ms} ms, outils acceptés (${test.tools_latency_ms} ms) — testé ${when}. La latence varie selon la charge du fournisseur.`,
        },
        `✓ ${formatSeconds(test.latency_ms)}`,
      );
    }
    if (test.ok) {
      return h("span", { class: "model-test warn", title: `${test.error} — testé ${when}` }, "⚠ sans outils");
    }
    return h("span", { class: "model-test bad", title: `${test.error} — testé ${when}` }, "✗ échec");
  }

  function badge(label: string, title: string, kind = ""): HTMLElement {
    return h("span", { class: `badge ${kind}`, title }, label);
  }

  function buildRow(model: ModelInfo): HTMLElement {
    const isMain = model.id === mainModel;
    const isVoice = model.id === voiceModel;
    const test = tests[model.id];
    const broken = Boolean(test && !test.ok);
    const busy = testing.has(model.id);
    const capabilities: HTMLElement[] = [];
    if (model.reasoning) capabilities.push(badge("raisonne", "Réfléchit avant de répondre : plus lent mais plus juste"));
    if (model.vision) capabilities.push(badge("vision", "Comprend les images"));
    if (model.tool_call === false) capabilities.push(badge("sans outils", "Le catalogue indique qu'il n'appelle pas d'outils", "warn"));
    if (!model.in_catalog) capabilities.push(badge("hors catalogue", "Le compte l'autorise mais le catalogue public ne le décrit pas"));

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
          isMain ? badge("principal", "Modèle utilisé pour le chat et les tâches", "main") : null,
          isVoice ? badge("vocal", "Modèle utilisé pour les échanges à voix haute", "voice") : null,
        ),
        h("code", {}, model.id),
        model.description ? h("p", { class: "model-desc", title: model.description }, model.description) : null,
      ),
      h(
        "div",
        { class: "model-meta" },
        h("span", { title: "Fenêtre de contexte" }, `contexte ${formatContext(model.context)}`),
        h("span", { title: "Prix en dollars par million de jetons (entrée / sortie)" }, formatCost(model)),
        h("div", { class: "badges" }, ...capabilities),
      ),
      status(model),
      h(
        "div",
        { class: "model-actions" },
        h(
          "button",
          { class: "ghost small", disabled: busy, onclick: () => void runTest(model.id), title: "Envoie une petite requête de test" },
          "Tester",
        ),
        h(
          "button",
          {
            class: isMain ? "primary small" : "ghost small",
            disabled: busy || isMain || broken,
            title: broken ? "Ce modèle a échoué au test" : "Utiliser pour le chat et les tâches",
            onclick: () => void choose("main", model.id),
          },
          "Principal",
        ),
        h(
          "button",
          {
            class: isVoice ? "primary small" : "ghost small",
            disabled: busy || isVoice || broken,
            title: broken ? "Ce modèle a échoué au test" : "Utiliser pour les échanges à voix haute",
            onclick: () => void choose("voice", model.id),
          },
          "Vocal",
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
          const t = tests[m.id];
          return Boolean(t?.ok && t.tools);
        }
        return true;
      }),
    );
  }

  function rank(m: ModelInfo): number {
    if (m.id === mainModel) return 0;
    if (m.id === voiceModel) return 1;
    const t = tests[m.id];
    if (t?.ok && t.tools) return 2;
    if (!t) return 3;
    return t.ok ? 4 : 5;
  }

  function sorted(items: ModelInfo[]): ModelInfo[] {
    const byName = (a: ModelInfo, b: ModelInfo) => a.name.localeCompare(b.name, "fr");
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
          const ta = tests[a.id]?.ok ? tests[a.id].latency_ms : Infinity;
          const tb = tests[b.id]?.ok ? tests[b.id].latency_ms : Infinity;
          return ta - tb || byName(a, b);
        });
      default:
        return copy.sort((a, b) => {
          const delta = rank(a) - rank(b);
          if (delta) return delta;
          const ta = tests[a.id]?.ok ? tests[a.id].latency_ms : Infinity;
          const tb = tests[b.id]?.ok ? tests[b.id].latency_ms : Infinity;
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
          h("p", {}, models.length ? "Aucun modèle ne correspond." : "Aucun modèle disponible."),
          onlyWorking ? h("p", { class: "hint" }, "Lance « Tester tous les modèles » pour savoir lesquels fonctionnent.") : null,
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

  renderCurrent();
  void load();

  return h(
    "section",
    { class: "card models-panel" },
    h("h3", {}, "Modèles de langage — bibliothèque OpenCode Go"),
    h(
      "p",
      { class: "note" },
      "La liste vient de ton compte. « Tester » envoie une petite requête, comme Jimmy le fait (puis une avec outils) : un modèle ne peut être choisi que s'il répond. Le choix s'applique tout de suite. La latence varie selon la charge du fournisseur : le test est une indication, pas une garantie.",
    ),
    current,
    h(
      "div",
      { class: "model-toolbar" },
      searchInput,
      sortSelect,
      h("label", { class: "toggle" }, workingBox, h("span", {}, "Seulement ceux qui fonctionnent")),
    ),
    h("div", { class: "row" }, batchButton, refreshButton, progress),
    list,
  );
}
