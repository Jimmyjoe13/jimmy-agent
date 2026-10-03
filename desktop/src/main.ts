/**
 * Point d'entrée de l'interface de Jimmy.
 *
 * Une fenêtre, une barre latérale, huit vues. Le routeur tient dans un
 * dictionnaire : ajouter une vue est une entrée, pas une refonte.
 *
 * Règles de rendu (corrigées après audit) :
 * - une seule vue montée à la fois : `mount` remplace, il n'empile pas ;
 * - un seul écouteur d'événements agent, redistribué aux vues ; les
 *   abonnements d'une vue sont libérés quand on la quitte ;
 * - l'état de Jimmy est visible sur toutes les pages, dans la barre du haut.
 */
import { api, onAgentEvent, type AgentEvent } from "./api";
import type { AppContext, Route } from "./context";
import { chatView } from "./views/chat";
import { historyView } from "./views/history";
import { diagnosticView, memoryView, skillsView, skinView } from "./views/panels";
import { onboardingView, voiceView } from "./views/voice";
import { settingsView } from "./views/settings";
import { STATE_LABEL, attempt, capitalize, guard, h, mount, toast } from "./ui";
import "./styles.css";

const ROUTES: { id: Route; label: string; icon: string; subtitle: string }[] = [
  { id: "chat", label: "Chat", icon: "◉", subtitle: "Parle à Jimmy, ou écris-lui." },
  { id: "voice", label: "Voix", icon: "◍", subtitle: "Écoute permanente, reconnaissance et synthèse vocale." },
  { id: "history", label: "Historique", icon: "◷", subtitle: "Les sessions enregistrées sur cette machine." },
  { id: "memory", label: "Mémoire", icon: "❋", subtitle: "Ce que Jimmy a retenu de toi." },
  { id: "skills", label: "Skills", icon: "◆", subtitle: "Les procédures réutilisables de Jimmy." },
  { id: "skin", label: "Skin", icon: "☻", subtitle: "Apparence et qualité de l'avatar." },
  { id: "settings", label: "Paramètres", icon: "⚙", subtitle: "Modèle, voix, écoute, avatar, permissions." },
  { id: "diagnostic", label: "Diagnostic", icon: "✚", subtitle: "Vérifier que tout est en place." },
];

async function main() {
  const root = document.getElementById("app");
  if (!root) throw new Error("#app introuvable");

  const bootstrap = await guard(() => api.bootstrap(), "démarrage");
  if (!bootstrap) {
    mount(root, h("div", { class: "fatal" }, "Impossible de démarrer Jimmy. Voir data/logs/jimmy.log."));
    return;
  }

  let route: Route = "chat";
  // Abonnements de la vue courante, libérés à chaque navigation.
  let scope: (() => void)[] = [];
  const handlers = new Set<(event: AgentEvent) => void>();

  const ctx: AppContext = {
    status: bootstrap.status,
    voice: null,
    lastSessionId: null,
    navigate: (next) => {
      route = next;
      render();
    },
    refreshStatus: async () => {
      const [fresh, voice] = await Promise.all([
        guard(() => api.status(), "état"),
        api.voiceStatus().catch(() => null),
      ]);
      if (fresh) ctx.status = fresh;
      ctx.voice = voice;
      renderSidebar();
    },
    onEvent: (handler) => {
      handlers.add(handler);
      const unsubscribe = () => handlers.delete(handler);
      scope.push(unsubscribe);
      return unsubscribe;
    },
    onCleanup: (cleanup) => {
      scope.push(cleanup);
    },
  };

  const sidebar = h("aside", { class: "sidebar" });
  const header = h("header", { class: "topbar" });
  const content = h("main", { class: "content" });
  const stateChip = h("span", { class: "chip state-idle", title: "État de Jimmy" }, STATE_LABEL.idle);

  function setState(state: string) {
    stateChip.textContent = STATE_LABEL[state] ?? state;
    stateChip.className = `chip state-${state}`;
  }

  // Retour à « prêt » après une réponse : le chat n'émet pas d'état final,
  // la pastille restait sur « je parle ». Tout nouvel état annule le retour.
  let idleTimer = 0;

  // Un seul écouteur Tauri pour toute l'application.
  void onAgentEvent((event) => {
    if (event.type === "state") {
      window.clearTimeout(idleTimer);
      setState(event.state ?? "idle");
    }
    if (event.type === "final") {
      window.clearTimeout(idleTimer);
      idleTimer = window.setTimeout(() => setState("idle"), 6000);
    }
    if (event.type === "failed") {
      setState("error");
      toast(event.message ?? "Échec", "error");
    }
    if (event.type === "notice" && event.message) toast(event.message, "info");
    for (const handler of handlers) handler(event);
  });

  function renderSidebar() {
    const info = ctx.status;
    const voice = ctx.voice;
    const voiceLabel = info.tts.voices.find((v) => v.id === info.tts.voice)?.label ?? "aucune";
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
          h("span", { class: "brand-sub" }, `v${info.version}${info.dev ? " · dépôt local" : ""}`),
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
        statusLine("Voix", voiceLabel, info.tts.has_key),
        statusLine(
          "Écoute",
          voice?.running ? `active · « ${capitalize(voice.wakeWord)} »` : "arrêtée",
          Boolean(voice?.running),
        ),
        statusLine("Avatar", info.avatar.running ? "actif" : "arrêté", info.avatar.running),
        statusLine(
          "Mémoire",
          `${info.memory.count} souvenir${info.memory.count > 1 ? "s" : ""}`,
          info.memory.enabled,
        ),
        statusLine("Synaptiq", info.synaptiq.configured ? "connecté" : "inactif", info.synaptiq.configured),
        statusLine("Outils", String(info.tools.length), true),
      ),
    );
  }

  function statusLine(label: string, value: string, ok: boolean): HTMLElement {
    return h(
      "div",
      { class: `status-line ${ok ? "ok" : "warn"}`, title: `${label} : ${value}` },
      h("span", { class: "status-dot" }),
      h("span", { class: "status-label" }, label),
      h("span", { class: "status-value" }, value),
    );
  }

  function viewFor(current: Route): HTMLElement {
    switch (current) {
      case "chat":
        return chatView(ctx);
      case "voice":
        return voiceView(ctx);
      case "history":
        return historyView(ctx);
      case "memory":
        return memoryView(ctx);
      case "skills":
        return skillsView();
      case "skin":
        return skinView(ctx);
      case "settings":
        return settingsView(ctx);
      case "diagnostic":
        return diagnosticView(ctx);
      default:
        return h("div", { class: "empty" }, "Vue inconnue.");
    }
  }

  function render() {
    // Libère les abonnements de la vue précédente avant d'en monter une autre.
    for (const unsubscribe of scope) unsubscribe();
    scope = [];

    const entry = ROUTES.find((r) => r.id === route);
    renderSidebar();
    mount(
      header,
      h("div", {}, h("h1", {}, entry?.label ?? "Jimmy"), h("p", { class: "subtitle" }, entry?.subtitle ?? "")),
      h(
        "div",
        { class: "topbar-actions" },
        stateChip,
        h(
          "button",
          {
            class: "ghost",
            title: "Relire l'état et recharger cette page",
            onclick: async () => {
              await ctx.refreshStatus();
              render();
            },
          },
          "Actualiser",
        ),
      ),
    );
    // `mount` remplace le contenu : une seule vue à la fois.
    mount(content, viewFor(route));
    content.scrollTop = 0;
  }

  mount(root, sidebar, h("section", { class: "main" }, header, content));

  if (!ctx.status.first_run_done) {
    mount(
      root,
      h(
        "div",
        { class: "onboarding-shell" },
        onboardingView(ctx, async () => {
          toast("Onboarding terminé. Bonne conversation.");
          mount(root, sidebar, h("section", { class: "main" }, header, content));
          // L'onboarding est le moment du consentement : on enchaîne sur
          // l'écoute, sinon Jimmy reste muet jusqu'au passage par la vue Voix.
          if (await attempt(() => api.voiceStart(), "écoute")) {
            toast(`Écoute active — dis « ${capitalize(ctx.status.stt.wake_word)} ».`, "info");
          }
          await ctx.refreshStatus();
          render();
        }),
      ),
    );
    return;
  }

  await ctx.refreshStatus();
  render();
  // La barre latérale suit l'état réel (avatar lancé, écoute coupée…) sans
  // attendre un clic sur « Actualiser ».
  window.setInterval(() => void ctx.refreshStatus(), 15_000);
}

void main();
