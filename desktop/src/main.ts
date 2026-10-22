/**
 * Point d'entrée de l'interface de Jimy.
 *
 * Une fenêtre, une barre latérale, huit vues. Le routeur tient dans un
 * dictionnaire : ajouter une vue est une entrée, pas une refonte.
 *
 * Règles de rendu (corrigées après audit) :
 * - une seule vue montée à la fois : `mount` remplace, il n'empile pas ;
 * - un seul écouteur d'événements agent, redistribué aux vues ; les
 *   abonnements d'une vue sont libérés quand on la quitte ;
 * - l'état de Jimy est visible sur toutes les pages, dans la barre du haut
 *   (la « Visière », direction du 21 octobre) ;
 * - tous les textes passent par `t()` (français et anglais, `i18n/`).
 */
import { api, onAgentEvent, type AgentEvent } from "./api";
import type { AppContext, Route } from "./context";
import { chatView } from "./views/chat";
import { historyView } from "./views/history";
import { diagnosticView, memoryView, skillsView, skinView } from "./views/panels";
import { onboardingView, voiceView } from "./views/voice";
import { settingsView } from "./views/settings";
import { mountTitlebar } from "./titlebar";
import { attempt, capitalize, guard, h, icon, mount, phaseLabel, stateLabel, toast, type IconName } from "./ui";
import { setLanguage, t, type Key } from "./i18n";
import "./styles.css";

// Onglets : libellé et sous-titre traduits (`nav.<id>`, `nav.<id>.subtitle`).
const ROUTES: { id: Route; icon: IconName }[] = [
  { id: "chat", icon: "chat" },
  { id: "voice", icon: "voice" },
  { id: "memory", icon: "memory" },
  { id: "skills", icon: "skills" },
  { id: "skin", icon: "skin" },
  { id: "history", icon: "history" },
  { id: "settings", icon: "settings" },
  { id: "diagnostic", icon: "diagnostic" },
];
const routeLabel = (id: Route) => t(`nav.${id}` as Key);
const routeSubtitle = (id: Route) => t(`nav.${id}.subtitle` as Key);

async function main() {
  const root = document.getElementById("app");
  if (!root) throw new Error("#app introuvable");
  // Langue de l'interface avant tout rendu (réglage `ui.language`) ; sans
  // réglage lisible, celle de Windows.
  const settings = await api.getSettings().catch(() => null);
  setLanguage(settings?.ui.language);
  // Puis la barre de titre : la fenêtre n'a plus de barre native, ses
  // boutons doivent exister même si le démarrage échoue.
  mountTitlebar();

  const bootstrap = await guard(() => api.bootstrap(), t("guard.start"));
  if (!bootstrap) {
    mount(root, h("div", { class: "fatal" }, t("fatal.start")));
    return;
  }

  let route: Route = "chat";
  // Abonnements de la vue courante, libérés à chaque navigation.
  let scope: (() => void)[] = [];
  const handlers = new Set<(event: AgentEvent) => void>();

  // Ctrl+K : la recherche de sessions depuis n'importe quelle vue (façon
  // Codex). Un écouteur global unique, enregistré au démarrage : il ne
  // s'empile pas avec la navigation (piège 11).
  window.addEventListener("keydown", (event) => {
    if (event.ctrlKey && !event.shiftKey && !event.altKey && event.key.toLowerCase() === "k") {
      event.preventDefault();
      route = "history";
      render();
    }
    // Échap : arrêt d'urgence quand Jimy travaille ou parle. Un menu ou un
    // aperçu ouvert consomme Échap avant (`preventDefault`).
    if (event.key === "Escape" && !event.defaultPrevented && !globalStop.hidden) {
      event.preventDefault();
      void stopNow();
    }
  });

  const ctx: AppContext = {
    status: bootstrap.status,
    voice: null,
    lastSessionId: null,
    pendingProject: null,
    approvals: new Map(),
    taskTitles: new Map(),
    navigate: (next) => {
      route = next;
      render();
    },
    refreshStatus: async () => {
      const [fresh, voice] = await Promise.all([
        guard(() => api.status(), t("guard.status")),
        api.voiceStatus().catch(() => null),
      ]);
      if (fresh) ctx.status = fresh;
      ctx.voice = voice;
      renderSidebar();
      renderVisorControls();
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

  // Visière : l'état de Jimy en direct, en haut de chaque page. Le trait
  // lumineux s'anime quand il travaille (classe d'état, voir styles.css).
  const visorText = h("span", { class: "visor-text" }, stateLabel("idle"));
  const stateChip = h(
    "span",
    { class: "visor state-idle", role: "status", "aria-label": t("visor.label") },
    h("span", { class: "visor-glasses", "aria-hidden": "true" }),
    visorText,
    h("span", { class: "visor-line", "aria-hidden": "true" }),
  );
  // Cerveau, voix de Jimy et micro : réglables depuis n'importe quelle page.
  const visorControls = h("div", { class: "visor-controls" });

  // Arrêt d'urgence visible depuis toutes les vues, dès que Jimy travaille ou
  // parle (Chat comme voix). Même effet que « STOP », Échap ou Ctrl+Maj+Espace.
  const globalStop = h(
    "button",
    {
      class: "danger global-stop",
      hidden: true,
      title: t("visor.stopHint"),
      onclick: () => void stopNow(),
    },
    icon("stop", 15, 2),
    t("visor.stop"),
  );
  // États où Jimy est occupé au premier plan.
  const BUSY = new Set(["thinking", "executing", "speaking", "waiting", "transcribing"]);
  function showStop(state: string) {
    globalStop.hidden = !BUSY.has(state);
  }
  async function stopNow() {
    if (await attempt(() => api.agentStop(), t("guard.stop"))) toast(t("toast.stopAsked"), "info");
    globalStop.hidden = true;
  }

  // Compte à rebours de « à toi » : le temps qu'il reste pour parler.
  let countdown = 0;

  function setState(state: string) {
    window.clearInterval(countdown);
    visorText.textContent = stateLabel(state);
    stateChip.className = `visor state-${state}`;
    showStop(state);
  }

  /** Étape de l'écoute (événement `listen`) : plus fine que l'état de l'avatar. */
  function setPhase(phase: string, remaining: number) {
    window.clearInterval(countdown);
    window.clearTimeout(idleTimer);
    stateChip.className = `visor phase-${phase}`;
    showStop(phase);
    if (phase === "your_turn" && remaining > 0) {
      const end = Date.now() + remaining;
      const tick = () => {
        const seconds = Math.max(0, Math.ceil((end - Date.now()) / 1000));
        visorText.textContent = t("phase.yourTurnCountdown", { seconds });
      };
      tick();
      countdown = window.setInterval(tick, 250);
      return;
    }
    visorText.textContent = phaseLabel(phase);
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
    if (event.type === "listen") setPhase(event.phase ?? "idle", event.remaining ?? 0);
    if (event.type === "final") {
      window.clearTimeout(idleTimer);
      idleTimer = window.setTimeout(() => setState("idle"), 6000);
    }
    if (event.type === "failed") {
      setState("error");
      toast(event.message ?? t("toast.failed"), "error");
    }
    if (event.type === "notice" && event.message) toast(event.message, "info");
    // Conversation vocale : le Chat l'adopte, même si l'utilisateur est sur
    // un autre onglet à ce moment-là.
    if (event.type === "spoken" && event.sessionId) ctx.lastSessionId = event.sessionId;
    // Demande d'autorisation (fichier sensible) : Jimy est bloqué tant que
    // l'utilisateur n'a pas répondu. Hors du Chat, on le prévient et on montre
    // la fenêtre (souvent masquée derrière l'avatar).
    if (event.type === "approval" && event.id) {
      ctx.approvals.set(event.id, event);
      if (route !== "chat") {
        toast(t("toast.approvalInChat"), "info");
        void api.showMain().catch(() => undefined);
      }
    }
    if (event.type === "approvalResolved" && event.id) ctx.approvals.delete(event.id);
    // Fin du tour au premier plan : ses demandes n'ont plus d'objet (celles
    // d'une tâche de fond, marquées `taskId`, restent).
    if (event.type === "final" || event.type === "failed") {
      for (const [id, pending] of ctx.approvals) if (!pending.taskId) ctx.approvals.delete(id);
    }
    // Tâche passée en arrière-plan : Jimy est de nouveau disponible.
    if (event.type === "detached") {
      setState("idle");
      if (event.taskId) ctx.taskTitles.set(event.taskId, event.title ?? "");
    }
    if (event.type === "background" && event.event) {
      const inner = event.event;
      if (inner.type === "approval" && inner.id) {
        ctx.approvals.set(inner.id, { ...inner, taskId: event.taskId });
        if (route !== "chat") {
          toast(t("toast.bgApprovalInChat"), "info");
          void api.showMain().catch(() => undefined);
        }
      }
      if (inner.type === "approvalResolved" && inner.id) ctx.approvals.delete(inner.id);
      if (inner.type === "final" || inner.type === "failed") {
        for (const [id, pending] of ctx.approvals) if (pending.taskId === event.taskId) ctx.approvals.delete(id);
        // Dans le Chat, la vue s'en charge (bulle ou toast selon la session).
        if (route !== "chat") {
          if (inner.type === "final") toast(t("toast.bgDone", { title: ctx.taskTitles.get(event.taskId ?? "") ?? "" }), "info");
          else toast(t("toast.bgFailed", { message: inner.message ?? "" }), "error");
        }
      }
    }
    for (const handler of handlers) handler(event);
  });

  const SIDEBAR_KEY = "jimy.sidebar.collapsed";
  const narrowWindow = window.matchMedia("(max-width: 1180px)");
  let sidebarPref: boolean | null = (() => {
    try {
      const v = localStorage.getItem(SIDEBAR_KEY);
      return v === "1" ? true : v === "0" ? false : null;
    } catch {
      return null;
    }
  })();
  const isSidebarCollapsed = () => (sidebarPref !== null ? sidebarPref : narrowWindow.matches);
  const applySidebarState = () => {
    const collapsed = isSidebarCollapsed();
    root.classList.toggle("sidebar-collapsed", collapsed);
    sidebar.classList.toggle("collapsed", collapsed);
  };
  narrowWindow.addEventListener("change", () => {
    applySidebarState();
    renderSidebar();
  });

  function toggleSidebar() {
    const next = !isSidebarCollapsed();
    sidebarPref = next;
    try {
      localStorage.setItem(SIDEBAR_KEY, next ? "1" : "0");
    } catch {
      /* stockage indisponible */
    }
    applySidebarState();
    renderSidebar();
  }

  function renderSidebar() {
    applySidebarState();
    const collapsed = isSidebarCollapsed();
    const info = ctx.status;
    const voice = ctx.voice;
    const voiceLabel = info.tts.voices.find((v) => v.id === info.tts.voice)?.label ?? t("system.voiceNone");
    mount(
      sidebar,
      h(
        "div",
        { class: "brand" },
        // Logo officiel (monogramme « AJ » détouré), posé en fond CSS pour
        // que Vite l'embarque comme les polices.
        h("div", { class: "brand-mark", role: "img", "aria-label": "Agent Jimy" }),
        h(
          "div",
          { class: "brand-text" },
          h("strong", {}, "Jimy"),
          h("span", { class: "brand-sub" }, `v${info.version} · ${info.dev ? t("brand.devRepo") : t("brand.local")}`),
        ),
        h(
          "button",
          {
            type: "button",
            class: "sidebar-toggle",
            title: collapsed ? t("sidebar.expand") : t("sidebar.collapse"),
            "aria-label": collapsed ? t("sidebar.expand") : t("sidebar.collapse"),
            onclick: () => toggleSidebar(),
          },
          icon(collapsed ? "chevron-right" : "chevron-left", 16, 2.2),
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
              title: routeLabel(entry.id),
              onclick: () => ctx.navigate(entry.id),
            },
            h("span", { class: "nav-icon" }, icon(entry.icon, 20, 1.6)),
            h("span", { class: "nav-label" }, routeLabel(entry.id)),
          ),
        ),
      ),
      h(
        "div",
        { class: "sidebar-status" },
        // En-tête de la carte « Système » : la pastille de mise à jour y vit
        // (cliquable vers les Archives), sinon un simple « à jour ».
        h(
          "div",
          { class: "sidebar-status-head" },
          h("span", { class: "kicker" }, t("system.title")),
          info.update.pending
            ? h(
                "button",
                {
                  class: "update-badge",
                  title: t("system.updateHint"),
                  onclick: () => ctx.navigate("history"),
                },
                t("system.update"),
              )
            : h("span", { class: "uptodate-badge" }, t("system.upToDate")),
        ),
        statusLine(t("system.model"), info.llm.model, info.llm.has_key),
        statusLine(t("system.voice"), voiceLabel, info.tts.has_key),
        statusLine(
          t("system.listen"),
          voice?.running ? t("system.listenOn", { word: capitalize(voice.wakeWord) }) : t("system.listenOff"),
          Boolean(voice?.running),
        ),
        statusLine(t("system.avatar"), info.avatar.running ? t("system.on") : t("system.off"), info.avatar.running),
        statusLine(t("system.memory"), t("system.memories", { count: info.memory.count }), info.memory.enabled),
        statusLine(
          t("system.vault"),
          info.vault.enabled ? t("system.vaultNotes", { count: info.vault.notes }) : t("system.inactive"),
          info.vault.enabled,
        ),
        statusLine(t("system.tools"), String(info.tools.length), true),
      ),
    );
  }

  /** Cerveau, « Jimy parle », « Micro » : l'état réel, relu après chaque clic. */
  function renderVisorControls() {
    const info = ctx.status;
    const speaking = info.tts.enabled;
    const listening = Boolean(ctx.voice?.running);
    mount(
      visorControls,
      h(
        "button",
        { class: "visor-pill brain", title: t("visor.brainHint"), onclick: () => ctx.navigate("settings") },
        h("span", { class: "visor-pill-kicker" }, t("visor.brain")),
        info.llm.model,
      ),
      h(
        "button",
        {
          class: `visor-pill toggle-pill ${speaking ? "on" : ""}`,
          "aria-pressed": String(speaking),
          title: t("visor.speakHint"),
          onclick: async () => {
            if (await attempt(() => api.voiceSetEnabled("speech", !speaking), t("guard.voice"))) await ctx.refreshStatus();
          },
        },
        icon("speaker", 14, 2),
        speaking ? t("visor.speakOn") : t("visor.speakOff"),
      ),
      h(
        "button",
        {
          class: `visor-pill toggle-pill ${listening ? "on" : ""}`,
          "aria-pressed": String(listening),
          title: t("visor.micHint"),
          onclick: async () => {
            // Couper : l'écoute s'arrête et le réglage le retient. Rallumer :
            // le réglage d'abord, puis le démarrage de l'écoute.
            const ok = listening
              ? await attempt(() => api.voiceSetEnabled("listen", false), t("guard.listen"))
              : (await attempt(() => api.voiceSetEnabled("listen", true), t("guard.listen"))) &&
                (await attempt(() => api.voiceStart(), t("guard.listen")));
            if (ok) await ctx.refreshStatus();
          },
        },
        icon("mic", 14, 2),
        listening ? t("visor.micOn") : t("visor.micOff"),
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
        return h("div", { class: "empty" }, t("nav.unknown"));
    }
  }

  function render() {
    // Libère les abonnements de la vue précédente avant d'en monter une autre.
    for (const unsubscribe of scope) unsubscribe();
    scope = [];

    const entry = ROUTES.find((r) => r.id === route);
    renderSidebar();
    renderVisorControls();
    mount(
      header,
      h("div", {}, h("h1", {}, entry ? routeLabel(entry.id) : "Jimy"), h("p", { class: "subtitle" }, entry ? routeSubtitle(entry.id) : "")),
      h(
        "div",
        { class: "topbar-actions" },
        globalStop,
        stateChip,
        visorControls,
        h(
          "button",
          {
            class: "ghost icon-only",
            title: t("visor.refreshHint"),
            "aria-label": t("visor.refresh"),
            onclick: async () => {
              await ctx.refreshStatus();
              render();
            },
          },
          icon("refresh", 16, 1.8),
        ),
      ),
    );
    // Chat : en-tête réduit à une ligne et marges resserrées, le fil de
    // discussion prend la hauteur (le reste s'efface au repos).
    header.classList.toggle("compact", route === "chat");
    content.classList.toggle("content-chat", route === "chat");
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
          toast(t("toast.onboardingDone"));
          mount(root, sidebar, h("section", { class: "main" }, header, content));
          // L'onboarding est le moment du consentement : on enchaîne sur
          // l'écoute, sinon Jimy reste muet jusqu'au passage par la vue Voix.
          if (await attempt(() => api.voiceStart(), t("guard.listen"))) {
            toast(t("toast.listening", { word: capitalize(ctx.status.stt.wake_word) }), "info");
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
