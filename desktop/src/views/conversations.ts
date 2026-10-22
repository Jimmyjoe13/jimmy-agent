/**
 * Liste des conversations, à gauche du Chat (refonte « Visière », étape 1).
 *
 * Audit du 21 octobre : changer de conversation passait par l'Historique, et
 * rien ne disait qu'une réponse était arrivée ailleurs ou qu'une tâche
 * attendait un accord. Ici, chaque conversation montre son état : en cours,
 * en arrière-plan, attend ton accord, messages non lus, et la voix qui y va.
 *
 * La liste ne fait que montrer et ouvrir : la conversation affichée est tenue
 * par le Chat (`onOpen`), qui rappelle `refresh` à chaque événement utile.
 */
import { api, type AgentEvent, type Session, type TaskInfo } from "../api";
import { guard, h, icon, mount } from "../ui";
import { locale, t } from "../i18n";

/** Teintes de projet, prises dans le logo : un projet garde toujours la sienne. */
const PROJECT_HUES = ["#ff7a59", "#a78bfa", "#22d3ee", "#fbbf24", "#34d399", "#f472b6"];
/** Délai de frappe avant d'interroger la base. */
const SEARCH_DELAY_MS = 250;
const COLLAPSED_KEY = "jimy.rail.collapsed";

export interface RailOptions {
  /** Conversation affichée dans le Chat (null : nouvelle, pas encore créée). */
  activeId: () => string | null;
  /** Conversation où ira la prochaine commande vocale. */
  voiceId: () => string | null;
  /** Demandes d'autorisation en attente (toutes conversations). */
  approvals: () => AgentEvent[];
  onOpen: (id: string) => void;
  onNew: (project: string | null) => void;
}

export interface Rail {
  element: HTMLElement;
  /** Relit conversations et tâches (après un événement ou un envoi). */
  refresh: () => Promise<void>;
  /** Libère l'écoute de la largeur de fenêtre (vue quittée). */
  dispose: () => void;
}

/** Sous cette largeur de fenêtre, la liste se replie d'elle-même : la
 *  fenêtre par défaut (1 040 px) avec l'explorateur ouvert ne laissait au
 *  fil qu'environ 180 px (22 octobre). */
const NARROW_QUERY = "(max-width: 1279px)";

/** Nom affiché d'un chemin de projet : son dernier dossier. */
export function projectName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

/** Teinte stable d'un projet (même chemin, même couleur). */
export function projectHue(path: string | null | undefined): string {
  if (!path) return "var(--text-faint)";
  let hash = 0;
  for (const char of path.toLowerCase()) hash = (hash * 31 + char.charCodeAt(0)) >>> 0;
  return PROJECT_HUES[hash % PROJECT_HUES.length];
}

/** Groupe de date d'une conversation : aujourd'hui, hier, cette semaine, avant. */
function dayGroup(iso: string, now = new Date()): "today" | "yesterday" | "week" | "older" {
  const date = new Date(iso);
  const start = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
  const day = 86_400_000;
  if (date.getTime() >= start) return "today";
  if (date.getTime() >= start - day) return "yesterday";
  if (date.getTime() >= start - 6 * day) return "week";
  return "older";
}

/** Heure courte : « 16:20 », « hier », « lun. », « 12/10 ». */
function shortTime(iso: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return "";
  switch (dayGroup(iso)) {
    case "today":
      return date.toLocaleTimeString(locale(), { hour: "2-digit", minute: "2-digit" });
    case "yesterday":
      return t("rail.yesterday");
    case "week":
      return date.toLocaleDateString(locale(), { weekday: "short" });
    default:
      return date.toLocaleDateString(locale(), { day: "2-digit", month: "2-digit" });
  }
}

function loadCollapsed(): boolean {
  try {
    return localStorage.getItem(COLLAPSED_KEY) === "1";
  } catch {
    return false;
  }
}

function saveCollapsed(collapsed: boolean) {
  try {
    localStorage.setItem(COLLAPSED_KEY, collapsed ? "1" : "0");
  } catch {
    /* stockage indisponible : la préférence n'est simplement pas gardée */
  }
}

export function conversationRail(options: RailOptions): Rail {
  let sessions: Session[] = [];
  let tasks: TaskInfo[] = [];
  // Filtre de projet (chemin), null = tous.
  let filter: string | null = null;
  // Préférence de l'utilisateur (fenêtre large), mémorisée.
  let collapsed = loadCollapsed();
  // Fenêtre étroite : repliée d'office, sauf si l'utilisateur la déplie
  // (le temps de la vue, sans toucher à sa préférence).
  const narrow = window.matchMedia(NARROW_QUERY);
  let openWhileNarrow = false;
  const isCollapsed = () => (narrow.matches ? !openWhileNarrow : collapsed);
  const onWidth = () => {
    openWhileNarrow = false;
    render();
  };
  narrow.addEventListener("change", onWidth);
  let searchTimer: number | undefined;

  const element = h("aside", { class: "conv-rail", "aria-label": t("rail.title") });
  const search = h("input", {
    class: "conv-search",
    type: "search",
    placeholder: t("rail.search"),
    "aria-label": t("rail.search"),
  }) as HTMLInputElement;
  search.addEventListener("input", () => {
    window.clearTimeout(searchTimer);
    searchTimer = window.setTimeout(() => void refresh(), SEARCH_DELAY_MS);
  });

  /** État d'une conversation, du plus urgent au plus calme. */
  function statusOf(session: Session): { key: string; label: string } | null {
    const taskIds = new Set(tasks.filter((task) => task.sessionId === session.id).map((task) => task.id));
    const waiting = options
      .approvals()
      .some((event) => event.sessionId === session.id || (event.taskId !== undefined && taskIds.has(event.taskId)));
    if (waiting) return { key: "approval", label: t("rail.status.approval") };
    const mine = tasks.filter((task) => task.sessionId === session.id);
    if (mine.some((task) => !task.background)) return { key: "running", label: t("rail.status.running") };
    if (mine.some((task) => task.background)) return { key: "background", label: t("rail.status.background") };
    return null;
  }

  async function refresh() {
    const [list, running] = await Promise.all([
      guard(() => api.sessions(search.value.trim() || null, false), t("rail.title")),
      api.tasksList().catch(() => [] as TaskInfo[]),
    ]);
    if (list) sessions = list;
    tasks = running;
    render();
  }

  /** Projets proposés en filtre : les plus récents d'abord, trois au plus. */
  function recentProjects(): string[] {
    const seen: string[] = [];
    for (const session of sessions) {
      if (session.project && !seen.includes(session.project)) seen.push(session.project);
      if (seen.length === 3) break;
    }
    if (filter && !seen.includes(filter)) seen.push(filter);
    return seen;
  }

  function row(session: Session): HTMLElement {
    const active = session.id === options.activeId();
    const status = statusOf(session);
    const unread = session.unread ?? 0;
    const voice = session.id === options.voiceId();
    return h(
      "button",
      {
        class: `conv-row${active ? " active" : ""}${unread > 0 ? " unread" : ""}`,
        "aria-current": active ? "true" : "false",
        title: session.title,
        onclick: () => options.onOpen(session.id),
      },
      h("span", { class: "conv-dot", style: `background: ${projectHue(session.project)}`, "aria-hidden": "true" }),
      h(
        "span",
        { class: "conv-text" },
        h(
          "span",
          { class: "conv-line" },
          h("span", { class: "conv-title" }, session.title || t("history.untitled")),
          h("span", { class: "conv-time" }, shortTime(session.updatedAt)),
        ),
        status || voice || unread > 0
          ? h(
              "span",
              { class: "conv-meta" },
              status ? h("span", { class: `conv-status ${status.key}` }, status.label) : null,
              voice ? h("span", { class: "conv-status voice" }, icon("mic", 11, 2.4), t("rail.status.voice")) : null,
              unread > 0
                ? h("span", { class: "conv-unread", "aria-label": t("rail.unreadLabel", { count: unread }) }, String(unread))
                : null,
            )
          : null,
      ),
    );
  }

  function render() {
    element.classList.toggle("collapsed", isCollapsed());
    const visible = filter ? sessions.filter((s) => s.project === filter) : sessions;

    if (isCollapsed()) {
      mount(
        element,
        h(
          "button",
          { class: "conv-toggle", title: t("rail.expand"), "aria-label": t("rail.expand"), onclick: () => toggle() },
          icon("chevron-right", 16, 2.2),
        ),
        h(
          "button",
          { class: "conv-new-mini", title: t("rail.new"), "aria-label": t("rail.new"), onclick: () => options.onNew(filter) },
          icon("plus", 16, 2.4),
        ),
        ...visible.slice(0, 14).map((session) => {
          const unread = (session.unread ?? 0) > 0;
          const status = statusOf(session);
          return h(
            "button",
            {
              class: `conv-mini${session.id === options.activeId() ? " active" : ""}${unread ? " unread" : ""}${status ? ` ${status.key}` : ""}`,
              title: session.title,
              "aria-label": session.title,
              style: `--hue: ${projectHue(session.project)}`,
              onclick: () => options.onOpen(session.id),
            },
            (session.title || "?").charAt(0).toUpperCase(),
          );
        }),
      );
      return;
    }

    const groups: [string, Session[]][] = [];
    const pinned = visible.filter((s) => s.pinned);
    if (pinned.length) groups.push([t("history.pinned"), pinned]);
    for (const key of ["today", "yesterday", "week", "older"] as const) {
      const items = visible.filter((s) => !s.pinned && dayGroup(s.updatedAt) === key);
      if (items.length) groups.push([t(`rail.group.${key}`), items]);
    }

    const chip = (label: string, value: string | null) =>
      h(
        "button",
        {
          class: `conv-chip${filter === value ? " on" : ""}`,
          "aria-pressed": String(filter === value),
          title: value ?? label,
          onclick: () => {
            filter = value;
            render();
          },
        },
        value ? h("span", { class: "conv-dot", style: `background: ${projectHue(value)}`, "aria-hidden": "true" }) : null,
        label,
      );

    mount(
      element,
      h(
        "div",
        { class: "conv-head" },
        h("h2", {}, t("rail.title")),
        h(
          "button",
          { class: "conv-new", title: t("rail.newHint"), onclick: () => options.onNew(filter) },
          icon("plus", 14, 2.6),
          t("rail.new"),
        ),
        h(
          "button",
          { class: "conv-toggle", title: t("rail.collapse"), "aria-label": t("rail.collapse"), onclick: () => toggle() },
          icon("chevron-left", 16, 2.2),
        ),
      ),
      h("div", { class: "conv-tools" }, search, h("div", { class: "conv-chips", role: "group", "aria-label": t("rail.filter") }, chip(t("rail.all"), null), ...recentProjects().map((path) => chip(projectName(path), path)))),
      h(
        "div",
        { class: "conv-list" },
        ...groups.flatMap(([label, items]) => [h("div", { class: "conv-group" }, label), ...items.map(row)]),
        groups.length === 0 ? h("p", { class: "conv-empty" }, search.value.trim() ? t("history.noMatch") : t("rail.empty")) : null,
      ),
    );
  }

  function toggle() {
    if (narrow.matches) {
      // Fenêtre étroite : dépliée pour un temps, préférence intacte.
      openWhileNarrow = !openWhileNarrow;
    } else {
      collapsed = !collapsed;
      saveCollapsed(collapsed);
    }
    render();
  }

  render();
  void refresh();
  return { element, refresh, dispose: () => narrow.removeEventListener("change", onWidth) };
}
