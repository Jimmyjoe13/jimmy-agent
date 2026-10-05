/**
 * Vue Chat : l'écran principal, texte d'abord, vocal ensuite.
 *
 * Une conversation peut être rattachée à un **projet** (un dossier), à la
 * manière de Codex Desktop : Jimmy y travaille, et le panneau « Fichiers »
 * permet d'y naviguer sans quitter le chat.
 */
import { api, type AgentEvent, type ChatMessage, type MentionEntry, type ProjectInfo } from "../api";
import { attempt, capitalize, guard, h, mount, toast } from "../ui";
import type { AppContext } from "../context";
import { explorerPanel } from "./explorer";

/** Préférence d'affichage du panneau Fichiers (par machine). */
const FILES_KEY = "jimmy.chat.files";

function loadFilesOpen(): boolean {
  try {
    return localStorage.getItem(FILES_KEY) !== "0";
  } catch {
    return true;
  }
}

function saveFilesOpen(open: boolean) {
  try {
    localStorage.setItem(FILES_KEY, open ? "1" : "0");
  } catch {
    /* stockage indisponible : la préférence n'est simplement pas gardée */
  }
}

/** Délai de sécurité : au-delà, l'interface est libérée même sans réponse. */
const ANSWER_TIMEOUT_MS = 180_000;

export function chatView(ctx: AppContext): HTMLElement {
  let sessionId: string | null = ctx.lastSessionId;
  let pending: HTMLElement | null = null;
  let safety = 0;
  // Bulle en cours d'écriture en flux (`delta`) : la réponse s'affiche pendant
  // que le modèle génère, au lieu d'un bloc unique à la fin. Transitoire :
  // `final` la remplace par la réponse complète, un appel d'outil la résout
  // en ligne d'activité (ce qui était écrit n'était qu'une annonce).
  let streaming: HTMLElement | null = null;
  // Fichiers écrits par Jimmy dans le tour courant (`write_file`) : la
  // matière du bloc « travaux » ajouté sous la réponse finale.
  let turnFiles: string[] = [];
  // Projet de la conversation (null = dossier par défaut des Paramètres).
  // Une nouvelle conversation garde le projet en cours, comme un nouveau fil
  // dans le même projet chez Codex.
  let project: string | null = ctx.pendingProject;
  let defaultFolder: ProjectInfo | null = null;

  // ── Projet et explorateur ────────────────────────────────────────────────
  const explorer = explorerPanel();
  let filesOpen = loadFilesOpen();
  explorer.element.hidden = !filesOpen;
  const projectLabel = h("span", { class: "project-name" }, "…");
  const projectMenu = h("div", { class: "project-menu", hidden: true, role: "menu" });
  const projectButton = h(
    "button",
    {
      class: "ghost project-button",
      title: "Choisir le projet de cette conversation",
      "aria-haspopup": "menu",
      onclick: (event: Event) => {
        event.stopPropagation();
        void toggleProjectMenu();
      },
    },
    h("span", { class: "project-kicker" }, "Projet"),
    projectLabel,
    h("span", { class: "project-caret" }, "▾"),
  );
  const filesButton = h(
    "button",
    {
      class: `ghost files-button${filesOpen ? " active" : ""}`,
      title: "Afficher ou masquer les fichiers du projet",
      onclick: () => {
        filesOpen = !filesOpen;
        explorer.element.hidden = !filesOpen;
        filesButton.classList.toggle("active", filesOpen);
        saveFilesOpen(filesOpen);
      },
    },
    "Fichiers",
  );

  const nameOf = (path: string) => path.split(/[\\/]/).filter(Boolean).pop() ?? path;

  /** Met l'en-tête et l'explorateur au diapason du projet courant. */
  function renderProject() {
    const path = project ?? defaultFolder?.path ?? "";
    projectLabel.textContent = project ? nameOf(project) : "aucun (dossier par défaut)";
    projectButton.setAttribute("title", path ? `Jimmy travaille dans : ${path}` : "Choisir un projet");
    if (path) explorer.setRoot(path, project ? nameOf(project) : "Dossier par défaut");
  }

  /** Rattache `path` (ou rien) à la conversation ; appliqué au prochain message. */
  async function applyProject(path: string | null, readable = true) {
    projectMenu.hidden = true;
    // Commande sans retour : `attempt` (piège 13). Échec = on ne change rien.
    if (sessionId && !(await attempt(() => api.sessionSetProject(sessionId as string, path), "projet"))) return;
    project = path;
    // Pas encore de conversation : le choix doit survivre au changement d'onglet.
    if (!sessionId) ctx.pendingProject = path;
    renderProject();
    if (path && !readable) {
      toast("Jimmy n'a pas la permission de lire ce dossier : ajoute-le dans Paramètres → Permissions.", "error");
    } else if (path) {
      toast(`Projet : ${nameOf(path)}`);
    }
  }

  function projectItem(info: ProjectInfo, label?: string): HTMLElement {
    return h(
      "button",
      {
        class: `project-item${info.path === project ? " current" : ""}`,
        role: "menuitem",
        disabled: !info.exists,
        title: info.exists ? info.path : `${info.path} (introuvable)`,
        onclick: () => void applyProject(label ? null : info.path, info.readable),
      },
      h("strong", {}, label ?? info.name),
      h("span", { class: "row-meta" }, info.exists ? info.path : "dossier introuvable"),
    );
  }

  async function toggleProjectMenu() {
    if (!projectMenu.hidden) {
      projectMenu.hidden = true;
      return;
    }
    const data = await guard(() => api.projectsRecent(), "projets récents");
    if (!data) return;
    defaultFolder = data.default;
    const recent = data.recent.filter((p) => p.path !== data.default.path);
    mount(
      projectMenu,
      h(
        "button",
        {
          class: "project-item open",
          role: "menuitem",
          onclick: async () => {
            projectMenu.hidden = true;
            const picked = await guard(() => api.pickFolder(), "choix du dossier");
            if (picked) {
              const known = [...data.recent, data.default].find((p) => p.path === picked);
              await applyProject(picked, known?.readable ?? true);
            }
          },
        },
        h("strong", {}, "Ouvrir un dossier…"),
        h("span", { class: "row-meta" }, "Choisir un projet sur le disque"),
      ),
      recent.length ? h("div", { class: "project-sep" }, "Récents") : null,
      ...recent.map((p) => projectItem(p)),
      h("div", { class: "project-sep" }, "Sans projet"),
      projectItem(data.default, "Dossier par défaut"),
    );
    projectMenu.hidden = false;
  }

  const stream = h("div", { class: "stream", "aria-live": "polite" });
  // Journal d'activité (outils, mémoire, vault). Masqué en CSS tant qu'il
  // est vide. Avant, il n'était jamais inséré dans la page.
  const activity = h("div", { class: "activity", "aria-label": "Activité de Jimmy" });

  const input = h("textarea", {
    class: "composer-input",
    rows: 1,
    placeholder: "Dis à Jimmy ce qu'il doit faire…",
    "aria-label": "Message pour Jimmy",
  }) as HTMLTextAreaElement;
  const sendButton = h("button", { class: "primary", onclick: () => void submit() }, "Envoyer");
  // Arrêt d'urgence, visible pendant que Jimmy travaille (équivaut à « STOP »).
  const stopButton = h(
    "button",
    {
      class: "danger stop-button",
      hidden: true,
      title: "Arrêter la tâche en cours (comme dire « STOP »)",
      onclick: () => void attempt(() => api.agentStop(), "arrêt"),
    },
    "Arrêter",
  );
  const hint = h("span", { class: "composer-hint" }, "Entrée pour envoyer · Maj+Entrée pour un retour à la ligne · @ pour citer un fichier");

  // ── Mentions « @ » (façon Codex) ───────────────────────────────────────────
  // « @ » ouvre un menu des fichiers du projet ; les suivants du jeton
  // filtrera. Entrée, Tab ou clic insère le chemin relatif en texte : Jimmy
  // lit le chemin comme n'importe quel texte, l'interface n'envoie rien de
  // spécial. Menu dans le composer : le clic dehors le referme.
  const mentionMenu = h("div", { class: "mention-menu", hidden: true, role: "listbox" });
  let mentionItems: MentionEntry[] = [];
  let mentionIndex = -1;
  let mentionStart = -1;
  let mentionTimer = 0;
  let mentionSeq = 0;

  /** Le jeton « @suite » courant sous le curseur. Non nul seulement si
   * l'`@` commence un mot (début de ligne ou espace avant), sans espace
   * dans le jeton, et si une racine de projet est disponible. */
  function mentionToken(): { start: number; query: string } | null {
    const caret = input.selectionStart;
    if (caret == null || caret === 0) return null;
    const before = input.value.slice(0, caret);
    const at = before.lastIndexOf("@");
    if (at < 0) return null;
    if (at > 0 && !/\s/.test(before[at - 1])) return null;
    const token = before.slice(at + 1);
    if (/[\s@]/.test(token)) return null;
    const root = project ?? defaultFolder?.path ?? "";
    if (!root) return null;
    return { start: at, query: token };
  }

  function mentionClose() {
    mentionMenu.hidden = true;
    mentionItems = [];
    mentionIndex = -1;
    mentionStart = -1;
    window.clearTimeout(mentionTimer);
  }

  function mentionRender() {
    mount(mentionMenu);
    mentionMenu.hidden = mentionItems.length === 0;
    if (mentionMenu.hidden) return;
    for (const [i, item] of mentionItems.entries()) {
      const row = h(
        "button",
        {
          type: "button",
          class: `mention-item${i === mentionIndex ? " active" : ""}`,
          role: "option",
          onclick: () =>
            void mentionAccept(item),
        },
        h("span", { class: "mention-name" }, item.name),
        h("span", { class: "mention-path" }, item.dir ? `dossier  ${item.display}` : item.display),
      );
      mentionMenu.append(row);
    }
  }

  async function mentionFetch(start: number, query: string, seq: number) {
    const root = project ?? defaultFolder?.path ?? "";
    const got = await guard(() => api.fsSearch(root, query), "mentions");
    if (!got) return;
    // Abandon : le jeton a changé pendant la recherche.
    const current = mentionToken();
    if (!current || current.start !== start || current.query !== query || mentionSeq !== seq) return;
    mentionItems = got.entries;
    mentionIndex = 0;
    mentionRender();
  }

  function ensureSchedule() {
    window.clearTimeout(mentionTimer);
    mentionTimer = window.setTimeout(() => {
      const token = mentionToken();
      if (!token) return void mentionClose();
      mentionStart = token.start;
      mentionSeq += 1;
      void mentionFetch(token.start, token.query, mentionSeq);
    }, 120);
  }

  function mentionMove(delta: number) {
    if (mentionItems.length === 0) return;
    mentionIndex = (mentionIndex + delta + mentionItems.length) % mentionItems.length;
    for (const [i, row] of [...mentionMenu.querySelectorAll(".mention-item")].entries()) {
      row.classList.toggle("active", i === mentionIndex);
    }
    mentionMenu.querySelectorAll(".mention-item")[mentionIndex]?.scrollIntoView({ block: "nearest" });
  }

  async function mentionAccept(item: MentionEntry) {
    const before = input.value.slice(0, mentionStart);
    const after = input.value.slice(input.selectionStart ?? input.value.length);
    input.value = `${before}@${item.display} ${after}`;
    const caret = before.length + item.display.length + 2;
    input.focus();
    input.setSelectionRange(caret, caret);
    mentionClose();
    autosize();
  }


  /** La zone de saisie grandit avec le texte, jusqu'à 8 lignes environ. */
  function autosize() {
    input.style.height = "auto";
    input.style.height = `${Math.min(input.scrollHeight, 180)}px`;
  }

  function setBusy(busy: boolean) {
    stopButton.hidden = !busy;
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
    const paragraph = h("p") as HTMLParagraphElement;
    if (role === "assistant") renderPaths(paragraph, content);
    else paragraph.textContent = content;
    return h("div", { class: `bubble ${cls}` }, paragraph);
  }

  // ── Chemins cliquables (façon Codex) ───────────────────────────────────────
  // Les chemins de fichiers d'une réponse deviennent des puces cliquables :
  // un aperçu s'ouvre (la même commande que le panneau Fichiers), et
  // « chemin:42 » ouvre le fichier en surlignant la ligne. Un exécutable
  // n'est jamais lancé depuis l'interface (règle de l'explorateur).

  /** Chemin absolu Windows (C:\…), ou relatif avec au moins un « / » ;
   * extension de 1 à 6 lettres ; « :ligne » en option. */
  const PATH_RE = new RegExp(
    [
      //                        groupe 1 : chemin  2 ext  3 ligne
      "([A-Za-z]:\\\\[^\\s`\"'<>|]+?)\\.(\\w{1,6})(?::(\\d{1,4}))?\\b",
      //                        groupe 4 : chemin  5 ext  6 ligne
      "((?:\\.{0,2}[\\/])?(?:[\\w.-]+[\\/])+[\\w.-]+)\\.(\\w{1,6})(?::(\\d{1,4}))?\\b",
    ].join("|"),
    "g",
  );

  /** Découpe `content` en texte + puces de chemin. Un candidat au milieu d'une
   * URL (https://…) reste du texte : ce n'est pas un fichier. */
  function renderPaths(paragraph: HTMLElement, content: string) {
    const flush = (text: string) => {
      if (text) paragraph.append(document.createTextNode(text));
    };
    let cursor = 0;
    PATH_RE.lastIndex = 0;
    for (let m = PATH_RE.exec(content); m !== null; m = PATH_RE.exec(content)) {
      const isAbsolute = m[1] !== undefined;
      const stem = isAbsolute ? m[1] : m[4];
      const ext = isAbsolute ? m[2] : m[5];
      const lineText = (isAbsolute ? m[3] : m[6]) ?? "";
      const path = `${stem}.${ext}`;
      const span = m.index ?? cursor;
      // URL en amont : candidat d'une adresse web, pas un fichier.
      if (content.slice(Math.max(0, span - 8), span + 2).includes("://")) {
        continue;
      }
      flush(content.slice(cursor, span));
      addPathChip(paragraph, `${path}${lineText}`, path, lineText ? Number(lineText) : undefined);
      cursor = span + path.length + lineText.length;
    }
    flush(content.slice(cursor));
  }

  /** Une puce de chemin dans une bulle ; le clic ouvre l'aperçu. */
  function addPathChip(paragraph: HTMLElement, label: string, path: string, line?: number) {
    paragraph.append(document.createTextNode(" "));
    const chip = h("button", { type: "button", class: "msg-path", title: "Aperçu du fichier" }, label);
    chip.addEventListener("click", () => void openFileModal(path, line));
    paragraph.append(chip);
  }

  /** Résout un chemin relatif par rapport au projet courant. */
  function resolvePath(path: string): string {
    if (path.length >= 2 && path[1] === ":") return path;
    const root = project ?? defaultFolder?.path ?? "";
    if (!root) return path;
    return root.replace(/[\\/]+$/, "") + "\\" + path.replace(/^[\\/]+/, "").replace(/\//g, "\\");
  }

  /** L'aperçu : une fenêtre par clic, recréée à chaque fois (pas d'état global
   * à synchroniser avec les vues ; nettoyée quand la vue est quittée). */
  let fileModal: HTMLElement | null = null;
  ctx.onCleanup(() => fileModal?.remove());

  async function openFileModal(path: string, line?: number) {
    const clean = resolvePath(path);
    const data = await guard(() => api.fsPreview(clean), "aperçu");
    fileModal?.remove();
    if (!data) return;
    const body = h("div", { class: "file-modal-body" });
    if (data.binary) {
      body.append(h("p", { class: "file-modal-note" }, "Fichier binaire : pas d'aperçu texte."));
    } else if (data.executable) {
      body.append(
        h("p", { class: "file-modal-note" }, "Exécutable : il n'est jamais lancé depuis le Chat, il est montré dans l'Explorateur."),
      );
      body.append(h("pre", { class: "file-modal-text" }, data.text || ""));
    } else {
      // Numérotation des lignes seulement si un « :ligne » a été demandé :
      // autrement, un texte brut suffit et reste léger.
      const target = data.text.split("\n");
      if (line && target.length <= 5000) {
        for (const [i, content] of target.entries()) {
          body.append(h("div", { class: `file-modal-line${i + 1 === line ? " target" : ""}` }, String(i + 1), content || " "));
        }
      } else {
        const pre = h("pre", { class: "file-modal-text" }, data.text || "");
        body.append(pre);
      }
    }
    const open = data.executable
      ? h(
          "button",
          { class: "ghost", title: "Montrer dans l'Explorateur (un exécutable n'est pas lancé)" },
          "Montrer dans l'Explorateur",
        ) as HTMLButtonElement
      : h("button", { class: "ghost", title: "Ouvrir avec l'application par défaut de Windows" }, "Ouvrir") as HTMLButtonElement;
    open.addEventListener("click", () => void attempt(() => api.fsOpen(clean), "ouverture"));
    fileModal = h(
      "div",
      {
        class: "file-modal",
        onclick: (event: Event) => {
          if (event.target === fileModal) closeModal();
        },
      },
      h(
        "div",
        { class: "file-modal-card" },
        h("div", { class: "file-modal-head" }, h("strong", {}, data.path), open, h("button", { class: "ghost", onclick: closeModal }, "Fermer")),
        body,
      ),
    );
    // Échap ferme l'aperçu (l'écouteur vit dans la fenêtre, retirée avec elle).
    fileModal.addEventListener("keydown", (event) => {
      if ((event as KeyboardEvent).key === "Escape") closeModal();
    });
    fileModal.setAttribute("tabindex", "-1");
    // Ajout AVANT le focus : un élément hors du document ne prend pas le
    // focus, Échap n'arrivait jamais à la fenêtre (restée ouverte, elle
    // bloquait ensuite tous les clics de l'interface).
    document.body.append(fileModal);
    (fileModal as HTMLElement).focus();
    if (line) {
      body.querySelector(".file-modal-line.target")?.scrollIntoView({ block: "center" });
    }
  }

  function closeModal() {
    fileModal?.remove();
    fileModal = null;
  }

  /** JSON d'arguments d'outil, tolérant (l'interface ne doit jamais planter
   * parce qu'un événement arrive dans une forme inattendue). */
  function safeParse(raw: string): unknown {
    try {
      return JSON.parse(raw);
    } catch {
      return null;
    }
  }

  // ── Bloc « travaux » (façon Codex) ─────────────────────────────────────────
  // Sous la réponse qui a écrit des fichiers : les chemins, en puces, et
  // pour chacun un diff git (lecture seule : on regarde, on ne valide pas
  // pour lui). Hors d'un dépôt, le diff reste silencieux.

  function workBlock(files: string[]): HTMLElement {
    const block = h("div", { class: "work-block" });
    block.append(h("div", { class: "work-kicker" }, `Travaux — ${files.length} fichier(s) écrit(s)`));
    for (const file of files) {
      const chip = h("button", { type: "button", class: "work-file", title: "Aperçu du fichier" }, nameOf(file));
      chip.addEventListener("click", () => void openFileModal(file));
      const diff = h("button", { type: "button", class: "ghost small work-diff", title: "Diff git du fichier" }, "Diff");
      diff.addEventListener("click", () => void showDiff(file));
      block.append(h("div", { class: "work-row" }, chip, diff));
    }
    return block;
  }

  /** Le diff git d'un fichier écrit pendant le tour. Dépôt absent, fichier
   * nouveau non commité ou lenteur : le message reste clair. */
  async function showDiff(file: string) {
    const data = await guard(() => api.fsDiff(file), "diff");
    if (!data) return;
    if (!data.diff) {
      toast(data.reason || "Diff git indisponible (fichier nouveau ou hors dépôt).");
      return;
    }
    fileModal?.remove();
    const body = h("div", { class: "file-modal-body" });
    for (const content of data.diff.split("\n")) {
      const cls = content.startsWith("+") ? " add" : content.startsWith("-") ? " del" : "";
      body.append(h("div", { class: `file-modal-line${cls}` }, content || " "));
    }
    fileModal = h(
      "div",
      { class: "file-modal", onclick: (event: Event) => event.target === fileModal && closeModal() },
      h(
        "div",
        { class: "file-modal-card" },
        h(
          "div",
          { class: "file-modal-head" },
          h("strong", {}, `${nameOf(file)} — diff git`),
          h("button", { class: "ghost", title: "Aperçu du fichier tel quel", onclick: () => void openFileModal(file) }, "Aperçu"),
          h("button", { class: "ghost", onclick: closeModal }, "Fermer"),
        ),
        body,
      ),
    );
    document.body.append(fileModal);
  }


  function append(node: HTMLElement) {
    clearEmptyState();
    stream.append(node);
    stream.scrollTop = stream.scrollHeight;
  }

  /** Remplace la bulle d'attente (ou en cours d'écriture) par la réponse. */
  function settle(node: HTMLElement) {
    if (pending) {
      pending.replaceWith(node);
      pending = null;
    } else if (streaming) {
      streaming.replaceWith(node);
      streaming = null;
    } else {
      append(node);
    }
    stream.scrollTop = stream.scrollHeight;
    setBusy(false);
  }

  /** La réponse s'écrit : la bulle d'attente devient la bulle en flux, ou une
   *  nouvelle bulle est ouverte. Retourne le paragraphe à garnir. */
  function streamingTarget(): HTMLElement {
    if (!streaming) {
      streaming = h("div", { class: "bubble assistant streaming" }, h("p", {}, ""));
      if (pending) {
        pending.replaceWith(streaming);
        pending = null;
      } else {
        append(streaming);
      }
    }
    return streaming.firstElementChild as HTMLElement;
  }

  /** Ce qui a été écrit en flux n'était qu'une annonce (« Je lis le dossier… ») :
   *  un outil part, la bulle se résout en ligne d'activité et une bulle
   *  d'attente reprend, comme avant le premier fragment. */
  function resolveStreamingAsAnnouncement() {
    if (!streaming) return;
    const announced = streaming.textContent?.trim() ?? "";
    streaming.remove();
    streaming = null;
    if (announced) {
      logActivity(activityLine("annonce", "note", h("span", { class: "activity-detail" }, announced)));
    }
    if (!pending) {
      pending = h(
        "div",
        { class: "bubble assistant pending" },
        h("p", {}, h("span", { class: "dots" }, h("i"), h("i"), h("i")), " Jimmy réfléchit"),
      );
      append(pending);
    }
  }

  /** Filet : sans réponse au bout de 3 minutes, la bulle d'attente devient une
   *  erreur. Suspendu pendant une demande d'autorisation (l'utilisateur peut
   *  prendre son temps), réarmé après sa réponse. */
  function armSafety() {
    window.clearTimeout(safety);
    safety = window.setTimeout(() => {
      if (pending || streaming) {
        settle(bubble("error", "Pas de réponse après 3 minutes. Réessaie, ou regarde le diagnostic."));
      }
    }, ANSWER_TIMEOUT_MS);
  }

  // Cartes d'autorisation ouvertes, par identifiant de demande.
  const approvalCards = new Map<string, HTMLElement>();

  /** Carte « Autoriser / Refuser » : Jimmy veut modifier un fichier sensible
   *  (`.env`, clés, secrets) et son outil est suspendu jusqu'à la réponse. */
  function showApproval(event: AgentEvent) {
    const id = event.id ?? "";
    if (!id || approvalCards.has(id)) return;
    const status = h("span", { class: "approval-status" }, "Jimmy attend ta réponse");
    const allow = h("button", { class: "primary small" }, "Autoriser") as HTMLButtonElement;
    const deny = h("button", { class: "danger small" }, "Refuser") as HTMLButtonElement;
    const answer = async (approved: boolean) => {
      allow.disabled = true;
      deny.disabled = true;
      const accepted = await guard(() => api.approvalRespond(id, approved), "autorisation");
      // Demande expirée ou tâche arrêtée entre-temps : rien n'a été exécuté.
      if (accepted === false) closeApproval(id, null);
    };
    allow.addEventListener("click", () => void answer(true));
    deny.addEventListener("click", () => void answer(false));
    const card = h(
      "div",
      { class: "bubble approval" },
      h("p", { class: "approval-title" }, "Autorisation demandée : modifier un fichier sensible"),
      h("code", { class: "approval-target" }, event.target ?? ""),
      h("pre", { class: "approval-detail" }, event.detail ?? ""),
      h("div", { class: "approval-actions" }, status, deny, allow),
    );
    approvalCards.set(id, card);
    clearEmptyState();
    // Au-dessus de la bulle d'attente : elle reste la dernière du fil.
    if (pending?.isConnected) pending.before(card);
    else append(card);
    stream.scrollTop = stream.scrollHeight;
    window.clearTimeout(safety);
  }

  /** Clôt une carte : accord, refus, ou `null` = expirée / sans objet. */
  function closeApproval(id: string, approved: boolean | null) {
    const card = approvalCards.get(id);
    if (!card) return;
    approvalCards.delete(id);
    card.querySelectorAll("button").forEach((button) => ((button as HTMLButtonElement).disabled = true));
    card.classList.add(approved ? "approved" : "denied");
    const status = card.querySelector(".approval-status");
    if (status) status.textContent = approved === null ? "Demande expirée : rien n'a été modifié" : approved ? "Autorisé" : "Refusé : rien n'a été modifié";
  }

  async function submit() {
    const text = input.value.trim();
    if (!text || pending || streaming) return;
    mentionClose();
    turnFiles = [];
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
    armSafety();

    const id = await guard(() => api.chat(sessionId, text, sessionId ? null : project), "envoi");
    if (!id) {
      settle(bubble("error", "Le message n'a pas pu être envoyé."));
      return;
    }
    sessionId = id;
    ctx.lastSessionId = id;
  }

  input.addEventListener("keydown", (event) => {
    // Le menu « @ » a la priorité : Entrée/Tab insère, Échap ferme.
    if (!mentionMenu.hidden) {
      if (event.key === "ArrowDown") {
        event.preventDefault();
        mentionMove(1);
        return;
      }
      if (event.key === "ArrowUp") {
        event.preventDefault();
        mentionMove(-1);
        return;
      }
      if (event.key === "Enter" || event.key === "Tab") {
        event.preventDefault();
        const item = mentionItems[mentionIndex] ?? null;
        if (item) void mentionAccept(item);
        else mentionClose();
        return;
      }
      if (event.key === "Escape") {
        event.preventDefault();
        mentionClose();
        return;
      }
    }
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      void submit();
    }
  });
  input.addEventListener("input", () => {
    autosize();
    ensureSchedule();
  });

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
        // Ce qui a été écrit en flux n'était qu'une annonce (« Je lis le
        // dossier… ») : un outil part, la bulle se résout en ligne d'activité.
        resolveStreamingAsAnnouncement();
        // Suivi des écritures du tour : la matière du bloc « travaux ».
        if (event.name === "write_file") {
          const args = typeof event.arguments === "string" ? safeParse(event.arguments) : event.arguments;
          const path = typeof (args as { path?: unknown } | null)?.path === "string" ? String((args as { path: string }).path) : null;
          if (path && !turnFiles.includes(path)) turnFiles.push(path);
        }
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
      case "skill":
        logActivity(activityLine("compétence", "memory", h("span", { class: "activity-detail" }, event.detail ?? "")));
        break;
      case "progress":
        // Étape d'une longue tâche vocale (aussi dite à voix haute).
        logActivity(activityLine("étape", "tool", h("span", { class: "activity-detail" }, event.text ?? "")));
        break;
      case "vault":
        logActivity(activityLine("Vault", "vault", h("span", { class: "activity-detail" }, event.detail ?? "")));
        break;
      case "spoken":
        // Commande vocale : affichée comme un message de l'utilisateur, avec
        // une bulle d'attente en dessous, comme pour un message écrit.
        if (event.sessionId) void adoptVoiceSession(event.sessionId);
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
      case "delta": {
        // Fragment reçu en flux : la bulle s'écrit au fil de la génération.
        const target = streamingTarget();
        target.append(event.text ?? "");
        stream.scrollTop = stream.scrollHeight;
        break;
      }
      case "approval":
        showApproval(event);
        break;
      case "approvalResolved":
        closeApproval(event.id ?? "", event.approved ?? false);
        if (pending || streaming) armSafety();
        break;
      case "final": {
        // Fin du tour (STOP compris) : une carte encore ouverte n'a plus d'objet.
        for (const id of [...approvalCards.keys()]) closeApproval(id, null);
        const node = bubble("assistant", event.text ?? "");
        if (turnFiles.length > 0) node.append(workBlock(turnFiles));
        settle(node);
        break;
      }
      case "failed":
        for (const id of [...approvalCards.keys()]) closeApproval(id, null);
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

  /** Projet de la conversation courante, lu dans la liste des sessions. */
  async function loadProject() {
    const data = await guard(() => api.projectsRecent(), "projets");
    if (data) defaultFolder = data.default;
    if (sessionId) {
      const sessions = await guard(() => api.sessions(), "sessions");
      project = sessions?.find((s) => s.id === sessionId)?.project ?? null;
    }
    renderProject();
  }

  /**
   * Conversation vocale qui démarre ou continue : le Chat l'adopte. Si elle
   * n'a pas de projet et qu'un projet est ouvert dans le Chat, il lui est
   * appliqué — Jimmy y travaille aussi à la voix.
   */
  async function adoptVoiceSession(id: string) {
    if (id === sessionId) return;
    const carried = project;
    sessionId = id;
    ctx.lastSessionId = id;
    await loadProject();
    if (!project && carried) await applyProject(carried);
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

  // Demandes arrivées pendant que l'utilisateur était sur un autre onglet :
  // affichées après l'historique (qui remplace le contenu du fil).
  void loadHistory().then(() => ctx.approvals.forEach((event) => showApproval(event)));
  void loadProject();
  window.setTimeout(() => input.focus(), 0);

  const chatHeader = h(
    "div",
    { class: "chat-header" },
    h("div", { class: "project-picker" }, projectButton, projectMenu),
    filesButton,
  );

  return h(
    "section",
    {
      class: "view chat",
      // Clic ailleurs : le menu des projets se ferme (écouteur porté par la
      // vue, détruit avec elle — pas d'écouteur global qui s'empile).
      onclick: (event: Event) => {
        if (!projectMenu.hidden && !projectMenu.contains(event.target as Node)) projectMenu.hidden = true;
        // Clic hors du menu des mentions : fermer (sauf clic dans le menu
        // lui-même, qui a déjà agi).
        if (!mentionMenu.hidden && !(mentionMenu as HTMLElement).contains(event.target as Node)) mentionClose();
      },
    },
    chatHeader,
    h(
      "div",
      { class: "chat-body" },
      h(
        "div",
        { class: "chat-main" },
    stream,
    activity,
    h(
      "div",
      { class: "composer" },
      mentionMenu,
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
              // Nouveau fil dans le même projet, comme chez Codex.
              ctx.pendingProject = project;
              mount(activity);
              void loadHistory();
            },
          },
          "Nouvelle session",
        ),
        stopButton,
        sendButton,
      ),
    ),
      ),
      explorer.element,
    ),
  );
}
