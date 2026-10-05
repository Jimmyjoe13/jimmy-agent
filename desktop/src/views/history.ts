/** Vue Historique : les sessions passées, groupées par projet, cherchables.
 *
 * Le groupement suit le projet de la conversation (à la manière de Codex :
 * tout ce qui touche à un projet se trouve au même endroit), et le champ de
 * recherche filtre en direct — `Ctrl+K` y arrive depuis n'importe quelle vue
 * (écouteur enregistré une fois, au démarrage, cf. `main.ts`).
 */
import { api, type Session } from "../api";
import { attempt, formatTime, h, mount, guard } from "../ui";
import type { AppContext } from "../context";

/** Nom projeté d'un chemin : le dernier composant (dossier ou fichier). */
function nameOf(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

export function historyView(ctx: AppContext): HTMLElement {
  let sessions: Session[] = [];

  const search = h("input", {
    class: "field history-search",
    placeholder: "Rechercher une session (titre ou projet)…",
    "aria-label": "Recherche de session",
  }) as HTMLInputElement;

  const list = h("div", { class: "list" });

  async function reload() {
    const data = await guard(() => api.sessions(), "historique");
    if (!data) return;
    sessions = data;
    render();
  }

  /** Session filtrée par le champ de recherche (casse ignorée). */
  function match(session: Session): boolean {
    const needle = search.value.trim().toLowerCase();
    if (!needle) return true;
    return session.title.toLowerCase().includes(needle) || (session.project ?? "").toLowerCase().includes(needle);
  }

  /** Un rendu par groupe de projet, du projet le plus récent, sessions
   * récentes d'abord. Les sessions sans projet tombent dans « Divers ». */
  function render() {
    mount(list);
    const visibles = sessions.filter(match);
    if (visibles.length === 0) {
      mount(
        list,
        h("div", { class: "empty" }, h("p", {}, sessions.length === 0 ? "Aucune session enregistrée." : "Aucune session ne correspond.")),
      );
      return;
    }
    const groups = new Map<string, Session[]>();
    for (const session of visibles) {
      const key = session.project || "";
      const bucket = groups.get(key) ?? [];
      bucket.push(session);
      groups.set(key, bucket);
    }
    for (const [project, bucket] of groups) {
      list.append(h("div", { class: "history-group", title: project || undefined }, project ? nameOf(project) : "Divers (sans projet)"));
      for (const session of bucket) list.append(row(session));
    }
  }

  function row(session: Session): HTMLElement {
    return h(
      "div",
      { class: "list-row" },
      h(
        "div",
        {},
        h("strong", {}, session.title || "Sans titre"),
        h(
          "div",
          { class: "row-meta" },
          `${formatTime(session.updatedAt)} · ${session.messageCount} message(s)`,
        ),
      ),
      h(
        "div",
        { class: "row-actions" },
        h(
          "button",
          {
            class: "ghost",
            onclick: () => {
              ctx.lastSessionId = session.id;
              ctx.navigate("chat");
            },
          },
          "Ouvrir",
        ),
        h(
          "button",
          {
            class: "danger",
            onclick: async () => {
              if (!window.confirm(`Supprimer la session « ${session.title || "Sans titre"} » ?`)) return;
              if (!(await attempt(() => api.deleteSession(session.id), "suppression"))) return;
              if (ctx.lastSessionId === session.id) {
                ctx.lastSessionId = null;
                ctx.pendingProject = session.project ?? null;
              }
              await reload();
            },
          },
          "Supprimer",
        ),
      ),
    );
  }

  search.addEventListener("input", render);
  void reload();
  // Le champ attend le clavier : l'utilisateur est arrivé ici pour chercher
  // (souvent par Ctrl+K), pas pour cliquer deux fois.
  window.setTimeout(() => search.focus(), 0);

  return h(
    "section",
    { class: "view" },
    h(
      "header",
      { class: "view-header" },
      h("p", { class: "note" }, "Tout est stocké sur cette machine, dans la base locale de Jimmy. Ctrl+K pour revenir ici à tout moment."),
      search,
      h("button", { class: "ghost", onclick: () => void reload() }, "Actualiser"),
    ),
    list,
  );
}
