/** Vues Mémoire, Skills, Skin et Diagnostic. */
import { api, type DoctorReport, type Memory, type Skill } from "../api";
import { formatTime, guard, h, mount, toast } from "../ui";
import type { AppContext } from "../context";
import { card } from "./settings";

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
            h("span", { class: `tag kind-${memory.kind}` }, memory.kind),
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
                  await guard(() => api.memoryForget(memory.id), "oubli");
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
      h("h2", {}, "Mémoire"),
      h("button", { class: "ghost", onclick: () => void reload() }, "Actualiser"),
    ),
    card(
      "Mémoire personnelle",
      h(
        "p",
        { class: "note" },
        "Stockée localement : vecteurs de 512 dimensions et index plein texte, sans service externe.",
        ctx.status.memory.has_fts ? " Recherche lexicale active." : " Recherche lexicale indisponible : seul le vecteur est utilisé.",
      ),
      h(
        "div",
        { class: "stat-row" },
        stat(String(ctx.status.memory.count), "souvenirs"),
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
      h("h2", {}, "Skills"),
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

/** Skin : le premier skin est un renard humanoïde ; d'autres pourront suivre. */
export function skinView(ctx: AppContext): HTMLElement {
  const qualityButtons = h(
    "div",
    { class: "row" },
    ...["low", "medium", "high"].map((level) =>
      h(
        "button",
        {
          class: ctx.status.avatar.quality === level ? "primary" : "ghost",
          onclick: async () => {
            await guard(() => api.avatarQuality(level), "qualité");
            await ctx.refreshStatus();
            toast(`Qualité « ${level} » appliquée`);
          },
        },
        level,
      ),
    ),
  );

  return h(
    "section",
    { class: "view" },
    h("header", { class: "view-header" }, h("h2", {}, "Skin")),
    card(
      "Skin actif",
      h("p", {}, h("strong", {}, "Renard humanoïde"), " — skin de base de la V1."),
      h(
        "p",
        { class: "note" },
        "Jimmy est l'identité ; le renard n'est qu'un apparence. L'architecture prévoit d'autres skins sans changer l'agent.",
      ),
    ),
    card("Qualité graphique", h("p", { class: "note" }, "Appliquée immédiatement à l'avatar Godot."), qualityButtons),
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
                  await guard(() => api.avatarState(state as never, ""), "état");
                },
              },
              state,
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
      h("input", { class: "field", id: "bubble-text", value: "Bonjour, je suis Jimmy." }) as HTMLElement,
      h(
        "button",
        {
          class: "ghost",
          onclick: () => {
            const input = document.getElementById("bubble-text") as HTMLInputElement | null;
            if (input) void guard(() => api.avatarSay(input.value), "bulle");
          },
        },
        "Afficher dans la bulle",
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
      h("h2", {}, "Diagnostic"),
      h(
        "div",
        { class: "row" },
        h("button", { class: "ghost", onclick: () => void showPaths() }, "Chemins"),
        h(
          "button",
          {
            class: "primary",
            onclick: async () => {
              await guard(() => api.reloadSecrets(), "secrets");
              toast("Secrets rechargés depuis .env");
              await ctx.refreshStatus();
              await run();
            },
          },
          "Recharger .env",
        ),
        h("button", { class: "ghost", onclick: () => void run() }, "Lancer"),
      ),
    ),
    card("Vérifications", report),
    card("Fichiers", paths),
  );
  void run();
  void showPaths();
  return container;
}