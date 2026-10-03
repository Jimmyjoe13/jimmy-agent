/** Vues Mémoire, Skills, Skin et Diagnostic. */
import { api, type DoctorReport, type Memory, type Skill } from "../api";
import { MEMORY_KIND_LABEL, QUALITY_LABEL, STATE_LABEL, attempt, formatTime, guard, h, mount, toast } from "../ui";
import type { AppContext } from "../context";
import { card, toggle } from "./settings";

/** Mémoire : ce que Jimmy a retenu, et le droit de l'oublier. */
export function memoryView(ctx: AppContext): HTMLElement {
  const container = h("section", { class: "view" });
  const list = h("div", { class: "list" });

  async function reload() {
    const memories = await guard(() => api.memoryList(), "mémoire");
    if (!memories) return;
    if (memories.length === 0) {
      mount(
        list,
        h(
          "div",
          { class: "empty" },
          h("p", {}, "La mémoire est vide pour l'instant."),
          h("p", { class: "hint" }, "Jimmy extrait des préférences et des règles au fil des échanges."),
        ),
      );
      return;
    }
    mount(list);
    for (const memory of memories as Memory[]) {
      list.append(
        h(
          "div",
          { class: "list-row" },
          h(
            "div",
            {},
            h("span", { class: `tag kind-${memory.kind}` }, MEMORY_KIND_LABEL[memory.kind] ?? memory.kind),
            h("p", {}, memory.content),
            h(
              "div",
              { class: "row-meta" },
              `${formatTime(memory.createdAt)} · importance ${memory.importance.toFixed(2)} · utilisée ${memory.useCount} fois`,
            ),
          ),
          h(
            "div",
            { class: "row-actions" },
            h(
              "button",
              {
                class: "danger",
                onclick: async () => {
                  if (!window.confirm("Oublier ce souvenir ? Jimmy ne pourra plus s'en servir.")) return;
                  if (!(await attempt(() => api.memoryForget(memory.id), "oubli"))) return;
                  await ctx.refreshStatus();
                  await reload();
                },
              },
              "Oublier",
            ),
          ),
        ),
      );
    }
  }

  mount(
    container,
    h(
      "header",
      { class: "view-header" },
      h("button", { class: "ghost", onclick: () => void reload() }, "Actualiser"),
    ),
    card(
      "Mémoire personnelle",
      h(
        "p",
        { class: "note" },
        "Stockée sur cette machine. ",
        ctx.status.memory.semantic_model
          ? "Recherche par le sens via LM Studio (repli automatique sur la recherche par mots si LM Studio est éteint)."
          : "Recherche par mots (aucun modèle d'embeddings configuré).",
      ),
      h(
        "div",
        { class: "stat-row" },
        stat(String(ctx.status.memory.count), ctx.status.memory.count > 1 ? "souvenirs" : "souvenir"),
        stat(String(ctx.status.tools.length), "outils"),
        stat(String(ctx.status.skills), "skills"),
      ),
      list,
    ),
  );
  void reload();
  return container;
}

function stat(value: string, label: string): HTMLElement {
  return h("div", { class: "stat" }, h("strong", {}, value), h("span", {}, label));
}

/** Skills : les procédures que Jimmy sait appliquer, et qu'il peut créer. */
export function skillsView(): HTMLElement {
  const container = h("section", { class: "view" });
  const list = h("div", { class: "list" });

  async function reload() {
    const skills = await guard(() => api.skillsList(), "skills");
    if (!skills) return;
    if (skills.length === 0) {
      mount(
        list,
        h(
          "div",
          { class: "empty" },
          h("p", {}, "Aucun skill."),
          h(
            "p",
            { class: "hint" },
            "Dis à Jimmy : « la prochaine fois, fais toujours X comme ça ». Il en créera un lui-même.",
          ),
        ),
      );
      return;
    }
    mount(list);
    for (const skill of skills as Skill[]) {
      list.append(
        h(
          "details",
          { class: "list-row skill" },
          h("summary", {}, h("strong", {}, skill.name)),
          h("p", { class: "note" }, skill.description),
          h("pre", {}, skill.body),
        ),
      );
    }
  }

  mount(
    container,
    h(
      "header",
      { class: "view-header" },
      h("button", { class: "ghost", onclick: () => void reload() }, "Actualiser"),
    ),
    card(
      "Répertoire",
      h(
        "p",
        { class: "note" },
        "Un skill par dossier, avec un fichier SKILL.md. Jimmy peut en créer et en améliorer lui-même.",
      ),
      list,
    ),
  );
  void reload();
  return container;
}

/** Skin : choix de l'apparence de Jimmy, appliqué à chaud par Godot. */
export function skinView(ctx: AppContext): HTMLElement {
  const skinButtons = h(
    "div",
    { class: "row" },
    ...ctx.status.avatar.skins.map((skin) =>
      h(
        "button",
        {
          class: ctx.status.avatar.skin === skin.id ? "primary" : "ghost",
          onclick: async () => {
            // `attempt` et non `guard` : la commande ne renvoie rien (piège 13).
            if (!(await attempt(() => api.avatarSkin(skin.id), "skin"))) return;
            await ctx.refreshStatus();
            // Re-rendu : le bouton du skin actif doit changer d'apparence.
            ctx.navigate("skin");
            toast(`Skin « ${skin.label} » appliqué`);
          },
        },
        skin.label,
      ),
    ),
  );

  const qualityButtons = h(
    "div",
    { class: "row" },
    ...["low", "medium", "high"].map((level) =>
      h(
        "button",
        {
          class: ctx.status.avatar.quality === level ? "primary" : "ghost",
          onclick: async () => {
            if (!(await attempt(() => api.avatarQuality(level), "qualité"))) return;
            await ctx.refreshStatus();
            ctx.navigate("skin");
            toast(`Qualité « ${QUALITY_LABEL[level]} » appliquée`);
          },
        },
        QUALITY_LABEL[level],
      ),
    ),
  );

  const bubbleInput = h("input", { class: "field", value: "Bonjour, je suis Jimmy." }) as HTMLInputElement;
  const dodgeToggle = h("input", { type: "checkbox" }) as HTMLInputElement;
  dodgeToggle.checked = ctx.status.avatar.dodge;
  dodgeToggle.addEventListener("change", async () => {
    const enabled = dodgeToggle.checked;
    if (!(await attempt(() => api.avatarDodge(enabled), "esquive"))) {
      dodgeToggle.checked = !enabled;
      return;
    }
    await ctx.refreshStatus();
    toast(enabled ? "Jimmy s'écartera à l'approche de la souris" : "Esquive désactivée");
  });

  return h(
    "section",
    { class: "view" },
    ctx.status.avatar.running
      ? null
      : h(
          "div",
          { class: "warn-note" },
          "L'avatar est arrêté : les changements seront appliqués à son prochain démarrage (Paramètres → Avatar).",
        ),
    card(
      "Skin actif",
      skinButtons,
      h(
        "p",
        { class: "note" },
        "Jimmy est l'identité, le skin n'est qu'une apparence : le changer ne touche ni à la mémoire ni aux réglages.",
      ),
    ),
    card("Qualité graphique", h("p", { class: "note" }, "Appliquée immédiatement à l'avatar Godot."), qualityButtons),
    card(
      "Comportement",
      h(
        "p",
        { class: "note" },
        "Seul le personnage capte la souris : le reste de sa fenêtre laisse passer les clics vers le bureau. Avec l'esquive, Jimmy s'écarte quand le curseur approche et revient à sa place ensuite — va le chercher là où il s'est réfugié pour le cliquer.",
      ),
      toggle("S'écarter à l'approche de la souris", dodgeToggle),
    ),
    card(
      "États de l'avatar",
      h(
        "div",
        { class: "state-grid" },
        ...["idle", "listening", "thinking", "speaking", "executing", "success", "error", "waiting"].map(
          (state) =>
            h(
              "button",
              {
                class: "state-chip",
                onclick: async () => {
                  await attempt(() => api.avatarState(state as never, ""), "état");
                },
              },
              STATE_LABEL[state] ?? state,
            ),
        ),
      ),
      h(
        "p",
        { class: "note" },
        "Clique sur un état pour le vérifier immédiatement sur l'avatar.",
      ),
    ),
    card(
      "Test de la bulle",
      h(
        "div",
        { class: "row" },
        bubbleInput,
        h(
          "button",
          { class: "ghost", onclick: () => void attempt(() => api.avatarSay(bubbleInput.value), "bulle") },
          "Afficher dans la bulle",
        ),
      ),
    ),
  );
}

/** Diagnostic : ce qui est prêt, ce qui manque, où sont les fichiers. */
export function diagnosticView(ctx: AppContext): HTMLElement {
  const container = h("section", { class: "view" });
  const report = h("div", { class: "list" });
  const paths = h("pre", { class: "paths" }, "…");

  async function run() {
    const result = await guard(() => api.doctor(), "diagnostic");
    mount(report);
    if (!result) {
      report.append(h("div", { class: "empty" }, h("p", {}, "Diagnostic impossible.")));
      return;
    }
    const typed = result as DoctorReport;
    report.append(
      h(
        "p",
        { class: typed.ok ? "ok-note" : "warn-note" },
        `${typed.passed} vérification(s) réussie(s) sur ${typed.total}.`,
      ),
    );
    for (const check of typed.checks) {
      report.append(
        h(
          "div",
          { class: `list-row check ${check.ok ? "ok" : "ko"}` },
          h("strong", {}, check.ok ? "✓" : "✗", " ", check.name),
          h("span", { class: "note" }, check.ok ? "" : check.hint),
        ),
      );
    }
  }

  async function showPaths() {
    const info = await guard(() => api.pathsInfo(), "chemins");
    if (!info) return;
    paths.textContent = JSON.stringify(info, null, 2);
  }

  mount(
    container,
    h(
      "header",
      { class: "view-header" },
      h(
        "div",
        { class: "row" },
        h(
          "button",
          {
            class: "ghost",
            onclick: async () => {
              const result = await guard(() => api.reloadSecrets(), "secrets");
              if (!result) return;
              toast(
                result.missing.length
                  ? `Secrets rechargés — manquants : ${result.missing.join(", ")}`
                  : "Secrets rechargés depuis .env",
                result.missing.length ? "error" : "info",
              );
              await ctx.refreshStatus();
              await run();
            },
          },
          "Recharger .env",
        ),
        h("button", { class: "primary", onclick: () => void run() }, "Relancer le diagnostic"),
      ),
    ),
    card("Vérifications", report),
    card("Fichiers", h("p", { class: "note" }, "Journal : data/logs/jimmy.log"), paths),
  );
  void run();
  void showPaths();
  return container;
}