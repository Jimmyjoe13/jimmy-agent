/** Vue Historique : les sessions passées, en local. */
import { api, type Session } from "../api";
import { attempt, formatTime, h, mount, guard } from "../ui";
import type { AppContext } from "../context";

export function historyView(ctx: AppContext): HTMLElement {
  const list = h("div", { class: "list" });

  async function reload() {
    const sessions = await guard(() => api.sessions(), "historique");
    if (!sessions) return;
    if (sessions.length === 0) {
      mount(list, h("div", { class: "empty" }, h("p", {}, "Aucune session enregistrée.")));
      return;
    }
    mount(list);
    for (const session of sessions as Session[]) {
      list.append(row(session));
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
              if (ctx.lastSessionId === session.id) ctx.lastSessionId = null;
              await reload();
            },
          },
          "Supprimer",
        ),
      ),
    );
  }

  void reload();

  return h(
    "section",
    { class: "view" },
    h(
      "header",
      { class: "view-header" },
      h("p", { class: "note" }, "Tout est stocké sur cette machine, dans la base locale de Jimmy."),
      h("button", { class: "ghost", onclick: () => void reload() }, "Actualiser"),
    ),
    list,
  );
}