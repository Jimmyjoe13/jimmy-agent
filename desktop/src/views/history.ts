/** Vue Historique : les sessions passées, groupées par projet, cherchables.
 *
 * Le groupement suit le projet de la conversation (à la manière de Codex :
 * tout ce qui touche à un projet se trouve au même endroit), et le champ de
 * recherche interroge la base en direct — `Ctrl+K` y arrive depuis n'importe
 * quelle vue (écouteur enregistré une fois, au démarrage, cf. `main.ts`).
 *
 * Audit du 21 octobre (lot A) : la liste ne montrait que les 50 dernières
 * sessions sur 195 et ne cherchait que dans le titre. La recherche porte
 * désormais sur toutes les sessions et le texte des échanges ; une session
 * s'épingle (en tête), se renomme (titre verrouillé) et s'archive.
 */
import { api, type Session } from "../api";
import { attempt, formatTime, h, mount, guard } from "../ui";
import type { AppContext } from "../context";
import { t } from "../i18n";

/** Nom projeté d'un chemin : le dernier composant (dossier ou fichier). */
function nameOf(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

/** Délai de frappe avant d'interroger la base. */
const SEARCH_DELAY_MS = 250;

export function historyView(ctx: AppContext): HTMLElement {
  let sessions: Session[] = [];
  // Liste affichée : sessions actives, ou archivées.
  let showArchived = false;
  let searchTimer: number | undefined;

  const search = h("input", {
    class: "field history-search",
    placeholder: t("history.search"),
    "aria-label": t("history.searchLabel"),
  }) as HTMLInputElement;

  const list = h("div", { class: "list" });

  const archivedToggle = h(
    "button",
    {
      class: "ghost history-archived-toggle",
      title: t("history.toggleHint"),
      onclick: () => {
        showArchived = !showArchived;
        archivedToggle.textContent = showArchived ? t("history.showActive") : t("history.showArchived");
        void reload();
      },
    },
    t("history.showArchived"),
  );

  /** Recherche côté base (toutes les sessions, contenu compris). */
  async function reload() {
    const query = search.value.trim();
    const data = await guard(() => api.sessions(query || null, showArchived), t("guard.history"));
    if (!data) return;
    sessions = data;
    render();
  }

  /** Épinglées en tête, puis un groupe par projet (le plus récent d'abord),
   * sessions récentes d'abord. Les sessions sans projet tombent dans « Divers ». */
  function render() {
    mount(list);
    if (sessions.length === 0) {
      const empty = search.value.trim()
        ? t("history.noMatch")
        : showArchived
          ? t("history.noArchived")
          : t("history.none");
      mount(list, h("div", { class: "empty" }, h("p", {}, empty)));
      return;
    }
    const pinned = sessions.filter((s) => s.pinned);
    if (pinned.length > 0) {
      list.append(h("div", { class: "history-group history-pinned" }, t("history.pinned")));
      for (const session of pinned) list.append(row(session));
    }
    const groups = new Map<string, Session[]>();
    for (const session of sessions.filter((s) => !s.pinned)) {
      const key = session.project || "";
      const bucket = groups.get(key) ?? [];
      bucket.push(session);
      groups.set(key, bucket);
    }
    for (const [project, bucket] of groups) {
      list.append(h("div", { class: "history-group", title: project || undefined }, project ? nameOf(project) : t("history.misc")));
      for (const session of bucket) list.append(row(session));
    }
  }

  /** Action d'une ligne : appel, puis liste rechargée si elle a abouti. */
  async function act(action: () => Promise<unknown>, label: string) {
    if (await attempt(action, label)) await reload();
  }

  function row(session: Session): HTMLElement {
    const title = session.title || t("history.untitled");
    return h(
      "div",
      { class: `list-row${session.pinned ? " pinned" : ""}`, "data-session": session.id },
      h(
        "div",
        {},
        h("strong", {}, title),
        h(
          "div",
          { class: "row-meta" },
          `${formatTime(session.updatedAt)} · ${t("history.messages", { count: session.messageCount })}${session.project && session.pinned ? ` · ${nameOf(session.project)}` : ""}`,
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
          t("history.open"),
        ),
        h(
          "button",
          {
            class: "ghost history-pin",
            title: session.pinned ? t("history.unpinHint") : t("history.pinHint"),
            onclick: () => void act(() => api.sessionPin(session.id, !session.pinned), t("guard.pin")),
          },
          session.pinned ? t("history.unpin") : t("history.pin"),
        ),
        h(
          "button",
          {
            class: "ghost history-rename",
            title: t("history.renameHint"),
            onclick: () => {
              const next = window.prompt(t("history.renamePrompt"), title);
              if (next === null || !next.trim() || next.trim() === title) return;
              void act(() => api.sessionRename(session.id, next.trim()), t("guard.rename"));
            },
          },
          t("history.rename"),
        ),
        h(
          "button",
          {
            class: "ghost history-archive",
            onclick: () => void act(() => api.sessionArchive(session.id, !session.archived), t("guard.archive")),
          },
          session.archived ? t("history.unarchive") : t("history.archive"),
        ),
        h(
          "button",
          {
            class: "danger",
            onclick: async () => {
              if (!window.confirm(t("history.deleteConfirm", { title }))) return;
              if (!(await attempt(() => api.deleteSession(session.id), t("guard.delete")))) return;
              if (ctx.lastSessionId === session.id) {
                ctx.lastSessionId = null;
                ctx.pendingProject = session.project ?? null;
              }
              await reload();
            },
          },
          t("history.delete"),
        ),
      ),
    );
  }

  // La base est interrogée après une courte pause de frappe, pas à chaque touche.
  search.addEventListener("input", () => {
    window.clearTimeout(searchTimer);
    searchTimer = window.setTimeout(() => void reload(), SEARCH_DELAY_MS);
  });
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
      h("p", { class: "note" }, t("history.note")),
      search,
      archivedToggle,
      h("button", { class: "ghost", onclick: () => void reload() }, t("history.refresh")),
    ),
    list,
  );
}
