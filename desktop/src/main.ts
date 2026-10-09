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
 * - l'état de Jimy est visible sur toutes les pages, dans la barre du haut.
 */
import { api, onAgentEvent, type AgentEvent } from "./api";
import type { AppContext, Route } from "./context";
import { chatView } from "./views/chat";
import { historyView } from "./views/history";
import { diagnosticView, memoryView, skillsView, skinView } from "./views/panels";
import { onboardingView, voiceView } from "./views/voice";
import { settingsView } from "./views/settings";
import { mountTitlebar } from "./titlebar";
import { PHASE_LABEL, STATE_LABEL, attempt, capitalize, guard, h, icon, mount, toast, type IconName } from "./ui";
import "./styles.css";

const ROUTES: { id: Route; label: string; icon: IconName; subtitle: string }[] = [
  { id: "chat", label: "Chat", icon: "chat", subtitle: "Parle à Jimy, ou écris-lui." },
  { id: "voice", label: "Voix", icon: "voice", subtitle: "Écoute permanente, reconnaissance et synthèse vocale." },
  { id: "history", label: "Historique", icon: "history", subtitle: "Les sessions enregistrées sur cette machine." },
  { id: "memory", label: "Mémoire", icon: "memory", subtitle: "Ce que Jimy a retenu de toi." },
  { id: "skills", label: "Skills", icon: "skills", subtitle: "Les procédures réutilisables de Jimy." },
  { id: "skin", label: "Skin", icon: "skin", subtitle: "Apparence et qualité de l'avatar." },
  { id: "settings", label: "Paramètres", icon: "settings", subtitle: "Modèle, voix, écoute, avatar, permissions." },
  { id: "diagnostic", label: "Diagnostic", icon: "diagnostic", subtitle: "Vérifier que tout est en place." },
];

async function main() {
  const root = document.getElementById("app");
  if (!root) throw new Error("#app introuvable");
  // Avant tout : la fenêtre n'a plus de barre native, ses boutons doivent
  // exister même si le démarrage échoue.
  mountTitlebar();

  const bootstrap = await guard(() => api.bootstrap(), "démarrage");
  if (!bootstrap) {
    mount(root, h("div", { class: "fatal" }, "Impossible de démarrer Jimy. Voir data/logs/jimmy.log."));
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
  const stateChip = h("span", { class: "chip state-idle", title: "État de Jimy" }, STATE_LABEL.idle);

  // Compte à rebours de « à toi » : le temps qu'il reste pour parler.
  let countdown = 0;

  function setState(state: string) {
    window.clearInterval(countdown);
    stateChip.textContent = STATE_LABEL[state] ?? state;
    stateChip.className = `chip state-${state}`;
  }

  /** Étape de l'écoute (événement `listen`) : plus fine que l'état de l'avatar. */
  function setPhase(phase: string, remaining: number) {
    window.clearInterval(countdown);
    window.clearTimeout(idleTimer);
    stateChip.className = `chip phase-${phase}`;
    if (phase === "your_turn" && remaining > 0) {
      const end = Date.now() + remaining;
      const tick = () => {
        const seconds = Math.max(0, Math.ceil((end - Date.now()) / 1000));
        stateChip.textContent = `à toi · ${seconds} s`;
      };
      tick();
      countdown = window.setInterval(tick, 250);
      return;
    }
    stateChip.textContent = PHASE_LABEL[phase] ?? phase;
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
      toast(event.message ?? "Échec", "error");
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
        toast("Jimy attend ton autorisation dans le Chat (fichier sensible).", "info");
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
          toast("La tâche de fond attend ton autorisation dans le Chat (fichier sensible).", "info");
          void api.showMain().catch(() => undefined);
        }
      }
      if (inner.type === "approvalResolved" && inner.id) ctx.approvals.delete(inner.id);
      if (inner.type === "final" || inner.type === "failed") {
        for (const [id, pending] of ctx.approvals) if (pending.taskId === event.taskId) ctx.approvals.delete(id);
        // Dans le Chat, la vue s'en charge (bulle ou toast selon la session).
        if (route !== "chat") {
          if (inner.type === "final") toast(`Tâche de fond terminée : ${ctx.taskTitles.get(event.taskId ?? "") ?? ""}`, "info");
          else toast(`Tâche de fond en échec : ${inner.message ?? ""}`, "error");
        }
      }
    }
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
        // Logo officiel (monogramme « AJ » détouré), posé en fond CSS pour
        // que Vite l'embarque comme les polices.
        h("div", { class: "brand-mark", role: "img", "aria-label": "Agent Jimy" }),
        h(
          "div",
          { class: "brand-text" },
          h("strong", {}, "Jimy"),
          h("span", { class: "brand-sub" }, `v${info.version} · ${info.dev ? "dépôt local" : "local"}`),
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
            h("span", { class: "nav-icon" }, icon(entry.icon, 20, 1.6)),
            entry.label,
          ),
        ),
      ),
      h(
        "div",
        { class: "sidebar-status" },
        // En-tête de la carte « Système » : la pastille de mise à jour y vit
        // (cliquable vers l'Historique), sinon un simple « à jour ».
        h(
          "div",
          { class: "sidebar-status-head" },
          h("span", { class: "kicker" }, "Système"),
          info.update.pending
            ? h(
                "button",
                {
                  class: "update-badge",
                  title: "Mise à jour disponible : tape /update dans le Chat",
                  onclick: () => ctx.navigate("history"),
                },
                "mise à jour",
              )
            : h("span", { class: "uptodate-badge" }, "à jour"),
        ),
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
        statusLine(
          "Vault Obsidian",
          info.vault.enabled ? `${info.vault.notes} notes` : "inactif",
          info.vault.enabled,
        ),
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
      h("div", {}, h("h1", {}, entry?.label ?? "Jimy"), h("p", { class: "subtitle" }, entry?.subtitle ?? "")),
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
          icon("refresh", 16, 1.8),
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
          // l'écoute, sinon Jimy reste muet jusqu'au passage par la vue Voix.
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
