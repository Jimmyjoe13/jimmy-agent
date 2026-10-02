/** Vue Chat : l'écran principal, texte d'abord, vocal ensuite. */
import { api, onAgentEvent, type AgentEvent, type ChatMessage } from "../api";
import { h, mount, toast, guard } from "../ui";
import type { AppContext } from "../context";

const STATE_LABEL: Record<string, string> = {
  idle: "prêt",
  listening: "j'écoute",
  thinking: "je réfléchis",
  speaking: "je parle",
  executing: "j'agis",
  success: "c'est fait",
  error: "il y a un souci",
  waiting: "j'attends",
};

export function chatView(ctx: AppContext): HTMLElement {
  let sessionId: string | null = ctx.lastSessionId;
  let busy = false;
  const stream = h("div", { class: "stream" });
  const stateChip = h("span", { class: "chip", title: "État de Jimmy" }, "prêt");
  const log = h("div", { class: "activity" });

  const input = h("textarea", {
    class: "composer-input",
    rows: 2,
    placeholder: "Dis à Jimmy ce qu'il doit faire…  (Entrée pour envoyer, Maj+Entrée pour un retour à la ligne)",
  }) as HTMLTextAreaElement;

  const sendButton = h("button", { class: "primary", onclick: () => void submit() }, "Envoyer");

  async function submit() {
    const text = input.value.trim();
    if (!text || busy) return;
    busy = true;
    sendButton.setAttribute("disabled", "");
    input.value = "";
    pushMessage("user", text);
    stream.scrollTop = stream.scrollHeight;

    const id = await guard(() => api.chat(sessionId, text), "envoi");
    if (id) {
      sessionId = id;
      ctx.lastSessionId = id;
    }
    // La réponse arrive par le canal d'événements ; on libère l'interface dès
    // que l'agent a démarré, pas à la fin du traitement : l'utilisateur voit
    // l'avatar travailler pendant que la réponse se construit.
    window.setTimeout(() => {
      busy = false;
      sendButton.removeAttribute("disabled");
      input.focus();
    }, 400);
  }

  input.addEventListener("keydown", (event) => {
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      void submit();
    }
  });

  function pushMessage(role: string, content: string) {
    const bubble = h(
      "div",
      { class: `bubble ${role}` },
      role === "assistant" ? "Jimmy" : "Vous",
      h("p", {}, content),
    );
    stream.append(bubble);
    stream.scrollTop = stream.scrollHeight;
  }

  function setState(state: string) {
    stateChip.textContent = STATE_LABEL[state] ?? state;
    stateChip.className = `chip state-${state}`;
  }

  async function loadHistory() {
    const target = sessionId ?? ctx.lastSessionId;
    if (!target) {
      mount(
        stream,
        h(
          "div",
          { class: "empty" },
          h("p", {}, "Dis « Jimmy » pour lancer une commande, ou écris ici."),
          h(
            "p",
            { class: "hint" },
            "Exemple : « Jimmy, analyse ce dossier et explique-moi ce que tu trouves. »",
          ),
        ),
      );
      return;
    }
    const messages = await guard(() => api.sessionMessages(target), "historique");
    if (!messages) return;
    if (messages.length === 0) {
      mount(
        stream,
        h("div", { class: "empty" }, h("p", {}, "Nouvelle session. Que puis-je faire ?")),
      );
      return;
    }
    mount(stream);
    for (const message of messages as ChatMessage[]) pushMessage(message.role, message.content);
  }

  void loadHistory();
  ctx.onState(setState);
  ctx.onActivity((entry) => {
    log.prepend(entry);
    while (log.childElementCount > 40) log.lastElementChild?.remove();
  });

  return h(
    "section",
    { class: "view chat" },
    h(
      "header",
      { class: "view-header" },
      h("h2", {}, "Conversation"),
      stateChip,
    ),
    stream,
    log.childElementCount ? log : h("div", { class: "activity" }),
    h(
      "div",
      { class: "composer" },
      input,
      h(
        "div",
        { class: "composer-actions" },
        h(
          "button",
          {
            class: "ghost",
            title: "Tester la voix",
            onclick: () => ctx.navigate("voice"),
          },
          "Voix",
        ),
        h(
          "button",
          {
            class: "ghost",
            title: "Nouvel échange",
            onclick: () => {
              sessionId = null;
              ctx.lastSessionId = null;
              void loadHistory();
            },
          },
          "Nouvelle session",
        ),
        sendButton,
      ),
    ),
  );
}

/** Branchement global des événements agent → interface. */
export function attachAgentEvents(ctx: AppContext): void {
  void onAgentEvent((event: AgentEvent) => {
    switch (event.type) {
      case "state":
        ctx.emitState(event.state ?? "idle");
        break;
      case "toolStart":
        ctx.emitActivity(
          h(
            "div",
            { class: "activity-line tool" },
            h("span", { class: "activity-kind" }, "outil"),
            h("code", {}, event.name ?? "?"),
          ),
        );
        break;
      case "toolEnd":
        ctx.emitActivity(
          h(
            "div",
            { class: `activity-line ${event.ok ? "ok" : "ko"}` },
            h("span", { class: "activity-kind" }, event.ok ? "fait" : "échec"),
            h("code", {}, event.name ?? "?"),
            h("span", { class: "activity-detail" }, event.summary ?? ""),
            event.durationMs ? h("em", {}, `${event.durationMs} ms`) : null,
          ),
        );
        break;
      case "memory":
        ctx.emitActivity(
          h(
            "div",
            { class: "activity-line memory" },
            h("span", { class: "activity-kind" }, "mémoire"),
            h("span", { class: "activity-detail" }, event.detail ?? ""),
          ),
        );
        break;
      case "synaptiq":
        ctx.emitActivity(
          h(
            "div",
            { class: "activity-line synaptiq" },
            h("span", { class: "activity-kind" }, "Synaptiq"),
            h("span", { class: "activity-detail" }, event.detail ?? ""),
          ),
        );
        break;
      case "final":
        ctx.pushAssistant(event.text ?? "");
        break;
      case "notice":
        toast(event.message ?? "", "info");
        break;
      case "failed":
        toast(event.message ?? "Échec", "error");
        ctx.emitState("error");
        break;
      default:
        break;
    }
  });
}