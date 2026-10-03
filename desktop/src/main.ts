/**
 * Point d'entrée de l'interface de Jimmy.
 *
 * Une fenêtre, une barre latérale, sept vues. Le routeur tient dans un
 * dictionnaire : ajouter une vue est une entrée, pas une refonte.
 */
import { api, type Status } from "./api";
import type { AppContext, Route } from "./context";
import { attachAgentEvents, chatView } from "./views/chat";
import { historyView } from "./views/history";
import { diagnosticView, memoryView, skillsView, skinView } from "./views/panels";
import { onboardingView, voiceView } from "./views/voice";
import { settingsView } from "./views/settings";
import { guard, h, mount, toast } from "./ui";
import "./styles.css";

const ROUTES: { id: Route; label: string; icon: string }[] = [
  { id: "chat", label: "Chat", icon: "◉" },
  { id: "voice", label: "Voix", icon: "◍" },
  { id: "history", label: "Historique", icon: "◷" },
  { id: "memory", label: "Mémoire", icon: "❋" },
  { id: "skills", label: "Skills", icon: "◆" },
  { id: "skin", label: "Skin", icon: "☻" },
  { id: "settings", label: "Paramètres", icon: "⚙" },
  { id: "diagnostic", label: "Diagnostic", icon: "✚" },
];

async function main() {
  const root = document.getElementById("app");
  if (!root) throw new Error("#app introuvable");

  let status: Status;
  const bootstrap = await guard(() => api.bootstrap(), "démarrage");
  if (!bootstrap) {
    mount(root, h("div", { class: "fatal" }, "Impossible de démarrer Jimmy. Voir la console."));
    return;
  }
  status = bootstrap.status;

  // Canal commun aux vues.
  let route: Route = "chat";
  let lastSessionId: string | null = null;
  const stateHandlers: ((state: string) => void)[] = [];
  const activityHandlers: ((entry: HTMLElement) => void)[] = [];
  let assistantSink: ((text: string) => void) | null = null;

  const ctx: AppContext = {
    status,
    lastSessionId,
    navigate: (next) => {
      route = next;
      render();
    },
    refreshStatus: async () => {
      const fresh = await guard(() => api.status(), "état");
      if (fresh) {
        ctx.status = fresh;
        status = fresh;
        renderSidebarStatus();
      }
    },
    onState: (handler) => stateHandlers.push(handler),
    emitState: (state) => stateHandlers.forEach((handler) => handler(state)),
    onActivity: (handler) => activityHandlers.push(handler),
    emitActivity: (entry) => activityHandlers.forEach((handler) => handler(entry)),
    pushAssistant: (text) => assistantSink?.(text),
  };

  const sidebar = h("aside", { class: "sidebar" });
  const header = h("header", { class: "topbar" });
  const content = h("main", { class: "content" });

  function renderSidebarStatus() {
    const info = ctx.status;
    mount(
      sidebar,
      h(
        "div",
        { class: "brand" },
        h("div", { class: "brand-mark" }, "☻"),
        h(
          "div",
          {},
          h("strong", {}, "Jimmy"),
          h("span", { class: "brand-sub" }, info.dev ? "développement" : "installé"),
        ),
      ),
      h(
        "nav",
        { class: "nav" },
        ...ROUTES.map((entry) =>
          h(
            "button",
            {
              class: `nav-item ${route === entry.id ? "active" : ""}`,
              onclick: () => ctx.navigate(entry.id),
            },
            h("span", { class: "nav-icon" }, entry.icon),
            entry.label,
          ),
        ),
      ),
      h(
        "div",
        { class: "sidebar-status" },
        statusLine("Modèle", info.llm.model, info.llm.has_key),
        statusLine("Voix", info.tts.voice ? "configurée" : "aucune", info.tts.has_key),
        statusLine("Écoute", info.stt.model, true),
        statusLine("Avatar", info.avatar.running ? "actif" : "arrêté", true),
        statusLine("Mémoire", `${info.memory.count} souvenir(s)`, info.memory.enabled),
        statusLine("Synaptiq", info.synaptiq.configured ? info.synaptiq.base_url : "inactif", info.synaptiq.configured),
        statusLine("Outils", String(info.tools.length), true),
      ),
    );
  }

  function statusLine(label: string, value: string, ok: boolean): HTMLElement {
    return h(
      "div",
      { class: `status-line ${ok ? "ok" : "warn"}` },
      h("span", { class: "status-dot" }),
      h("span", { class: "status-label" }, label),
      h("span", { class: "status-value" }, value),
    );
  }

  function render() {
    renderSidebarStatus();
    mount(
      header,
      h(
        "div",
        {},
        h("h1", {}, ROUTES.find((r) => r.id === route)?.label ?? "Jimmy"),
        h(
          "p",
          { class: "subtitle" },
          route === "chat"
            ? "Parle à Jimmy, ou écris-lui."
            : subtitleFor(route),
        ),
      ),
      h(
        "div",
        { class: "topbar-actions" },
        h("span", { class: "version" }, `v${ctx.status.version}`),
        h(
          "button",
          { class: "ghost", onclick: () => void ctx.refreshStatus() },
          "Actualiser",
        ),
      ),
    );

    switch (route) {
      case "chat":
        content.append(chatView(ctx));
        break;
      case "voice":
        content.append(voiceView(ctx));
        break;
      case "history":
        content.append(historyView(ctx));
        break;
      case "memory":
        content.append(memoryView(ctx));
        break;
      case "skills":
        content.append(skillsView());
        break;
      case "skin":
        content.append(skinView(ctx));
        break;
      case "settings":
        content.append(settingsView(ctx));
        break;
      case "diagnostic":
        content.append(diagnosticView(ctx));
        break;
      default:
        content.append(h("div", { class: "empty" }, "Vue inconnue."));
    }
  }

  mount(root, sidebar, h("section", { class: "main" }, header, content));

  // Si la vue chat est ouverte, elle expose le point d'entrée des réponses.
  const originalNavigate = ctx.navigate;
  ctx.navigate = (next) => {
    originalNavigate(next);
  };
  attachAgentEvents(ctx);
  const sinkTimer = window.setInterval(() => {
    if (!assistantSink) {
      assistantSink = (text: string) => {
        const stream = document.querySelector(".stream");
        if (!stream) return;
        stream.append(
          h("div", { class: "bubble assistant" }, "Jimmy", h("p", {}, text)),
        );
        stream.scrollTop = stream.scrollHeight;
      };
    }
  }, 200);
  window.addEventListener("beforeunload", () => window.clearInterval(sinkTimer));

  if (!status.first_run_done) {
    mount(
      root,
      h("div", { class: "onboarding-shell" }, onboardingView(ctx, () => {
        toast("Onboarding terminé. Bonne conversation.");
        void ctx.refreshStatus();
        render();
        // L'onboarding est le moment du consentement : on enchaîne sur
        // l'écoute, sinon Jimmy reste muet jusqu'au passage par la vue Voix.
        void api.voiceStart()
          .then(() => toast("Écoute active — dis « Jimmy ».", "info"))
          .catch((error) => toast(`Écoute : ${String(error)}`, "error"));
      })),
    );
    return;
  }

  render();
}

function subtitleFor(route: Route): string {
  switch (route) {
    case "voice":
      return "Écouter, transcrire, écouter la réponse.";
    case "history":
      return "Les sessions enregistrées sur cette machine.";
    case "memory":
      return "Ce que Jimmy a retenu de toi.";
    case "skills":
      return "Les procédures réutilisables de Jimmy.";
    case "skin":
      return "Apparence et qualité de l'avatar.";
    case "settings":
      return "Modèle, voix, permissions, démarrage.";
    case "diagnostic":
      return "Vérifier que tout est en place.";
    default:
      return "";
  }
}

void main();