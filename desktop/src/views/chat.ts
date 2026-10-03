/** Vue Chat : l'écran principal, texte d'abord, vocal ensuite. */
import { api, type AgentEvent, type ChatMessage } from "../api";
import { capitalize, guard, h, mount } from "../ui";
import type { AppContext } from "../context";

/** Délai de sécurité : au-delà, l'interface est libérée même sans réponse. */
const ANSWER_TIMEOUT_MS = 180_000;

export function chatView(ctx: AppContext): HTMLElement {
  let sessionId: string | null = ctx.lastSessionId;
  let pending: HTMLElement | null = null;
  let safety = 0;

  const stream = h("div", { class: "stream", "aria-live": "polite" });
  // Journal d'activité (outils, mémoire, Synaptiq). Masqué en CSS tant qu'il
  // est vide. Avant, il n'était jamais inséré dans la page.
  const activity = h("div", { class: "activity", "aria-label": "Activité de Jimmy" });

  const input = h("textarea", {
    class: "composer-input",
    rows: 1,
    placeholder: "Dis à Jimmy ce qu'il doit faire…",
    "aria-label": "Message pour Jimmy",
  }) as HTMLTextAreaElement;
  const sendButton = h("button", { class: "primary", onclick: () => void submit() }, "Envoyer");
  const hint = h("span", { class: "composer-hint" }, "Entrée pour envoyer · Maj+Entrée pour un retour à la ligne");

  /** La zone de saisie grandit avec le texte, jusqu'à 8 lignes environ. */
  function autosize() {
    input.style.height = "auto";
    input.style.height = `${Math.min(input.scrollHeight, 180)}px`;
  }

  function setBusy(busy: boolean) {
    if (busy) {
      sendButton.setAttribute("disabled", "");
      sendButton.textContent = "Jimmy travaille…";
    } else {
      sendButton.removeAttribute("disabled");
      sendButton.textContent = "Envoyer";
      window.clearTimeout(safety);
    }
  }

  function clearEmptyState() {
    stream.querySelector(".empty")?.remove();
  }

  function bubble(role: "user" | "assistant" | "error" | "voice", content: string): HTMLElement {
    // Le libellé (« vous » / « jimmy ») vient du CSS : l'ajouter aussi en
    // texte l'affichait deux fois. « voice » : message de l'utilisateur dit
    // à voix haute (même place qu'un message écrit, repère micro).
    const cls = role === "voice" ? "user voice" : role;
    return h("div", { class: `bubble ${cls}` }, h("p", {}, content));
  }

  function append(node: HTMLElement) {
    clearEmptyState();
    stream.append(node);
    stream.scrollTop = stream.scrollHeight;
  }

  /** Remplace la bulle d'attente par la réponse (ou ajoute si aucune attente). */
  function settle(node: HTMLElement) {
    if (pending) {
      pending.replaceWith(node);
      pending = null;
    } else {
      append(node);
    }
    stream.scrollTop = stream.scrollHeight;
    setBusy(false);
  }

  async function submit() {
    const text = input.value.trim();
    if (!text || pending) return;
    input.value = "";
    autosize();
    append(bubble("user", text));
    pending = h(
      "div",
      { class: "bubble assistant pending" },
      h("p", {}, h("span", { class: "dots" }, h("i"), h("i"), h("i")), " Jimmy réfléchit"),
    );
    append(pending);
    setBusy(true);
    safety = window.setTimeout(() => {
      if (pending) settle(bubble("error", "Pas de réponse après 3 minutes. Réessaie, ou regarde le diagnostic."));
    }, ANSWER_TIMEOUT_MS);

    const id = await guard(() => api.chat(sessionId, text), "envoi");
    if (!id) {
      settle(bubble("error", "Le message n'a pas pu être envoyé."));
      return;
    }
    sessionId = id;
    ctx.lastSessionId = id;
  }

  input.addEventListener("keydown", (event) => {
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      void submit();
    }
  });
  input.addEventListener("input", autosize);

  function activityLine(kind: string, cls: string, ...rest: (Node | string | null)[]): HTMLElement {
    return h("div", { class: `activity-line ${cls}` }, h("span", { class: "activity-kind" }, kind), ...rest);
  }

  function logActivity(line: HTMLElement) {
    activity.prepend(line);
    while (activity.childElementCount > 40) activity.lastElementChild?.remove();
  }

  ctx.onEvent((event: AgentEvent) => {
    switch (event.type) {
      case "toolStart":
        logActivity(activityLine("outil", "tool", h("code", {}, event.name ?? "?")));
        break;
      case "toolEnd":
        logActivity(
          activityLine(
            event.ok ? "fait" : "échec",
            event.ok ? "ok" : "ko",
            h("code", {}, event.name ?? "?"),
            h("span", { class: "activity-detail" }, event.summary ?? ""),
            event.durationMs ? h("em", {}, `${event.durationMs} ms`) : null,
          ),
        );
        break;
      case "memory":
        logActivity(activityLine("mémoire", "memory", h("span", { class: "activity-detail" }, event.detail ?? "")));
        break;
      case "synaptiq":
        logActivity(activityLine("Synaptiq", "synaptiq", h("span", { class: "activity-detail" }, event.detail ?? "")));
        break;
      case "spoken":
        // Commande vocale : affichée comme un message de l'utilisateur, avec
        // une bulle d'attente en dessous, comme pour un message écrit.
        append(bubble("voice", event.text ?? ""));
        if (!pending) {
          pending = h(
            "div",
            { class: "bubble assistant pending" },
            h("p", {}, h("span", { class: "dots" }, h("i"), h("i"), h("i")), " Jimmy réfléchit"),
          );
          append(pending);
        }
        break;
      case "final":
        settle(bubble("assistant", event.text ?? ""));
        break;
      case "failed":
        settle(bubble("error", event.message ?? "La demande a échoué."));
        break;
      default:
        break;
    }
  });

  function emptyState() {
    mount(
      stream,
      h(
        "div",
        { class: "empty" },
        h("p", {}, `Dis « ${capitalize(ctx.status.stt.wake_word)} » pour lancer une commande, ou écris ici.`),
        h("p", { class: "hint" }, "Exemple : « Jimmy, analyse ce dossier et explique-moi ce que tu trouves. »"),
      ),
    );
  }

  async function loadHistory() {
    if (!sessionId) {
      emptyState();
      return;
    }
    const messages = await guard(() => api.sessionMessages(sessionId as string), "historique");
    if (!messages || messages.length === 0) {
      emptyState();
      return;
    }
    mount(stream);
    for (const message of messages as ChatMessage[]) {
      if (message.role === "user" || message.role === "assistant") {
        stream.append(bubble(message.role, message.content));
      }
    }
    stream.scrollTop = stream.scrollHeight;
  }

  void loadHistory();
  window.setTimeout(() => input.focus(), 0);

  return h(
    "section",
    { class: "view chat" },
    stream,
    activity,
    h(
      "div",
      { class: "composer" },
      input,
      h(
        "div",
        { class: "composer-actions" },
        hint,
        h(
          "button",
          {
            class: "ghost",
            title: "Commencer une nouvelle conversation",
            onclick: () => {
              sessionId = null;
              ctx.lastSessionId = null;
              mount(activity);
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
