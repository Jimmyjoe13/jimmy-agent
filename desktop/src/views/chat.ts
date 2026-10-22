/**
 * Vue Chat : l'écran principal, texte d'abord, vocal ensuite.
 *
 * Une conversation peut être rattachée à un **projet** (un dossier), à la
 * manière de Codex Desktop : Jimy y travaille, et le panneau « Fichiers »
 * permet d'y naviguer sans quitter le chat.
 */
import { api, type AgentEvent, type ChatMessage, type MentionEntry, type ProjectInfo, type ScreenCapture, type TaskInfo } from "../api";
import { attempt, capitalize, guard, h, icon, mount, toast } from "../ui";
import type { AppContext } from "../context";
import { explorerPanel } from "./explorer";
import { conversationRail } from "./conversations";
import { t } from "../i18n";

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
  // Changements de conversation (ouverture, nouvelle) : un envoi terminé après
  // un changement ne ramène pas l'utilisateur en arrière.
  let navToken = 0;
  // Premier message d'une conversation neuve en cours d'envoi.
  let awaitingNewSession = false;
  let pending: HTMLElement | null = null;
  let safety = 0;
  // Filet armé ou non : `clearTimeout` seul ne suffit pas à le savoir.
  // Sans ce drapeau, impossible de distinguer « jamais armé » de « expiré ».
  let safetyOn = false;
  // Bulle en cours d'écriture en flux (`delta`) : la réponse s'affiche pendant
  // que le modèle génère, au lieu d'un bloc unique à la fin. Transitoire :
  // `final` la remplace par la réponse complète, un appel d'outil la résout
  // en ligne d'activité (ce qui était écrit n'était qu'une annonce).
  let streaming: HTMLElement | null = null;
  // Fichiers écrits par Jimy dans le tour courant (`write_file`) : la
  // matière du bloc « travaux » ajouté sous la réponse finale.
  let turnFiles: string[] = [];
  // Fichiers écrits par chaque tâche de fond : son bloc « travaux » à la fin.
  const backgroundFiles = new Map<string, string[]>();
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
      title: t("chat.project.choose"),
      "aria-haspopup": "menu",
      onclick: (event: Event) => {
        event.stopPropagation();
        void toggleProjectMenu();
      },
    },
    h("span", { class: "project-kicker" }, t("chat.project.kicker")),
    projectLabel,
    h("span", { class: "project-caret" }, icon("caret", 14, 2)),
  );
  const filesButton = h(
    "button",
    {
      class: `ghost files-button${filesOpen ? " active" : ""}`,
      title: t("chat.files.toggle"),
      onclick: () => {
        filesOpen = !filesOpen;
        explorer.element.hidden = !filesOpen;
        filesButton.classList.toggle("active", filesOpen);
        saveFilesOpen(filesOpen);
      },
    },
    icon("folder", 16),
    t("chat.files"),
  );

  const nameOf = (path: string) => path.split(/[\\/]/).filter(Boolean).pop() ?? path;

  /** Met l'en-tête et l'explorateur au diapason du projet courant. */
  function renderProject() {
    const path = project ?? defaultFolder?.path ?? "";
    projectLabel.textContent = project ? nameOf(project) : t("chat.project.none");
    projectButton.setAttribute("title", path ? t("chat.project.worksIn", { path }) : t("chat.project.pick"));
    if (path) explorer.setRoot(path, project ? nameOf(project) : t("chat.project.default"));
  }

  /** Rattache `path` (ou rien) à la conversation ; appliqué au prochain message. */
  async function applyProject(path: string | null, readable = true) {
    projectMenu.hidden = true;
    // Commande sans retour : `attempt` (piège 13). Échec = on ne change rien.
    if (sessionId && !(await attempt(() => api.sessionSetProject(sessionId as string, path), t("guard.project")))) return;
    project = path;
    // Pas encore de conversation : le choix doit survivre au changement d'onglet.
    if (!sessionId) ctx.pendingProject = path;
    renderProject();
    if (path && !readable) {
      toast(t("chat.project.noPermission"), "error");
    } else if (path) {
      toast(t("chat.project.set", { name: nameOf(path) }));
    }
  }

  function projectItem(info: ProjectInfo, label?: string): HTMLElement {
    return h(
      "button",
      {
        class: `project-item${info.path === project ? " current" : ""}`,
        role: "menuitem",
        disabled: !info.exists,
        title: info.exists ? info.path : t("chat.project.missing", { path: info.path }),
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
    const data = await guard(() => api.projectsRecent(), t("guard.recentProjects"));
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
            const picked = await guard(() => api.pickFolder(), t("guard.pickFolder"));
            if (picked) {
              const known = [...data.recent, data.default].find((p) => p.path === picked);
              await applyProject(picked, known?.readable ?? true);
            }
          },
        },
        h("strong", {}, t("chat.project.openFolder")),
        h("span", { class: "row-meta" }, t("chat.project.openFolderHint")),
      ),
      recent.length ? h("div", { class: "project-sep" }, t("chat.project.recent")) : null,
      ...recent.map((p) => projectItem(p)),
      h("div", { class: "project-sep" }, t("chat.project.noProject")),
      projectItem(data.default, t("chat.project.default")),
    );
    projectMenu.hidden = false;
  }

  const stream = h("div", { class: "stream", "aria-live": "polite" });
  // Journal d'activité (outils, mémoire, vault). Masqué en CSS tant qu'il
  // est vide. Avant, il n'était jamais inséré dans la page.
  const activity = h("div", { class: "activity", "aria-label": t("chat.activity") });

  const input = h("textarea", {
    class: "composer-input",
    rows: 1,
    placeholder: t("chat.placeholder"),
    "aria-label": t("chat.inputLabel"),
  }) as HTMLTextAreaElement;
  const sendLabel = h("span", { class: "send-label" }, t("chat.send"));
  const sendButton = h(
    "button",
    { class: "primary send-button", onclick: () => void submit() },
    icon("send", 17, 2.2),
    sendLabel,
  );
  // Arrêt d'urgence, visible pendant que Jimy travaille (équivaut à « STOP »).
  const stopButton = h(
    "button",
    {
      class: "danger stop-button",
      hidden: true,
      title: t("chat.stopHint"),
      onclick: () => void attempt(() => api.agentStop(), t("guard.stop")),
    },
    t("chat.stop"),
  );
  // ── Tâche de fond ────────────────────────────────────────────────────────
  // Bandeau au-dessus de la saisie : ce qui tourne en arrière-plan pendant
  // que la conversation continue, avec son étape et son propre arrêt
  // (« Arrêter » du composer et « STOP » n'arrêtent que le premier plan).
  const tasksBand = h("div", { class: "tasks-band", hidden: true, "aria-label": t("chat.bgTask") });
  let backgroundTask: TaskInfo | null = null;
  // Instant de lecture de `elapsedSecs` : la durée avance sans relire.
  let taskReadAt = 0;

  function renderTasks() {
    if (!backgroundTask) {
      tasksBand.hidden = true;
      mount(tasksBand);
      return;
    }
    const task = backgroundTask;
    const seconds = task.elapsedSecs + Math.floor((Date.now() - taskReadAt) / 1000);
    const duration = seconds < 60 ? `${seconds} s` : `${Math.floor(seconds / 60)} min`;
    tasksBand.hidden = false;
    mount(
      tasksBand,
      h("span", { class: "tasks-band-kind" }, t("chat.bgKind")),
      h("span", { class: "tasks-band-title", title: task.title }, task.title),
      h("span", { class: "tasks-band-step" }, task.step || t("chat.bgStarting")),
      h("em", { class: "tasks-band-time" }, duration),
      h(
        "button",
        {
          class: "danger small tasks-band-stop",
          title: t("chat.bgStopHint"),
          onclick: () => void stopBackground(task.id),
        },
        t("chat.stop"),
      ),
    );
  }

  /** Arrêt de la tâche de fond : relecture immédiate du registre, le bandeau
   *  disparaît même si la fin de la tâche n'arrive jamais par événement. */
  async function stopBackground(id: string) {
    const stopped = await guard(() => api.taskStop(id), t("guard.taskStop"));
    // `undefined` : échec, déjà signalé par `guard`.
    if (stopped !== undefined) toast(stopped ? t("chat.bgStopped") : t("chat.bgAlreadyDone"), "info");
    await refreshTasks();
  }

  async function refreshTasks() {
    const list = await guard(() => api.tasksList(), t("guard.tasks"));
    backgroundTask = list?.find((task) => task.background) ?? null;
    taskReadAt = Date.now();
    renderTasks();
  }

  // Bandeau tenu à jour depuis le registre (durée, fin manquée) ; minuterie
  // détruite avec la vue.
  const tasksTimer = window.setInterval(() => {
    if (backgroundTask) void refreshTasks();
  }, 5000);
  ctx.onCleanup(() => window.clearInterval(tasksTimer));

  // ── Vision : « Joindre ma fenêtre » ───────────────────────────────────────
  // Capture au clic (ce qui est montré est ce qui part), vignette retirable,
  // jointe au prochain message. Jimy ne capture jamais de lui-même.
  let capture: ScreenCapture | null = null;
  const captureChip = h("div", { class: "capture-chip", hidden: true });

  function renderCapture() {
    captureChip.hidden = !capture;
    if (!capture) return void mount(captureChip);
    const current = capture;
    mount(
      captureChip,
      h("img", { class: "capture-thumb", src: current.preview, alt: t("chat.capture.alt", { label: current.label }) }),
      h("span", { class: "capture-label", title: current.label }, t("chat.capture.attached", { label: current.label })),
      h(
        "button",
        {
          class: "ghost small",
          title: t("chat.capture.removeHint"),
          onclick: () => {
            capture = null;
            renderCapture();
            void attempt(() => api.screenDiscard(current.id), t("guard.capture"));
          },
        },
        t("chat.capture.remove"),
      ),
    );
  }

  const captureButton = h(
    "button",
    {
      class: "ghost capture-button",
      title: t("chat.capture.hint"),
      onclick: async () => {
        const got = await guard(() => api.screenCapture(), t("guard.capture"));
        if (!got) return;
        if (capture) void attempt(() => api.screenDiscard(capture!.id), t("guard.capture"));
        capture = got;
        renderCapture();
        input.focus();
      },
    },
    t("chat.capture.button"),
  );

  const hint = h(
    "span",
    { class: "composer-hint" },
    // Une seule ligne discrète sous la saisie (coupée si la place manque).
    h("kbd", {}, t("chat.hint.enter")),
    t("chat.hint.send"),
    h("kbd", {}, t("chat.hint.shiftEnter")),
    t("chat.hint.newline"),
    h("kbd", {}, "@"),
    t("chat.hint.mention"),
  );

  // Bouton « @ » du compositeur : insère le caractère au curseur et relance
  // la détection des mentions (même chemin que la frappe au clavier).
  const mentionButton = h(
    "button",
    {
      class: "composer-icon",
      type: "button",
      title: t("chat.mention.hint"),
      "aria-label": t("chat.mention.label"),
      onclick: () => {
        const at = input.selectionStart ?? input.value.length;
        const before = input.value.slice(0, at);
        // Un « @ » se colle à un mot précédent : on ajoute une espace avant.
        const glue = before && !/\s$/.test(before) ? " " : "";
        input.value = `${before}${glue}@${input.value.slice(input.selectionEnd ?? at)}`;
        const caret = at + glue.length + 1;
        input.setSelectionRange(caret, caret);
        input.focus();
        input.dispatchEvent(new Event("input"));
      },
    },
    icon("at", 19),
  );

  // ── Mentions « @ » (façon Codex) ───────────────────────────────────────────
  // « @ » ouvre un menu des fichiers du projet ; les suivants du jeton
  // filtrera. Entrée, Tab ou clic insère le chemin relatif en texte : Jimy
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
        h("span", { class: "mention-path" }, item.dir ? `${t("chat.mention.folder")}  ${item.display}` : item.display),
      );
      mentionMenu.append(row);
    }
  }

  async function mentionFetch(start: number, query: string, seq: number) {
    const root = project ?? defaultFolder?.path ?? "";
    const got = await guard(() => api.fsSearch(root, query), t("guard.mentions"));
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
    input.style.overflowY = input.scrollHeight > 180 ? "auto" : "hidden";
  }

  function setBusy(busy: boolean) {
    stopButton.hidden = !busy;
    if (busy) {
      sendButton.setAttribute("disabled", "");
      sendLabel.textContent = t("chat.working");
    } else {
      sendButton.removeAttribute("disabled");
      sendLabel.textContent = t("chat.send");
      window.clearTimeout(safety);
      safetyOn = false;
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
    const chip = h("button", { type: "button", class: "msg-path", title: t("chat.preview") }, label);
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
    const data = await guard(() => api.fsPreview(clean), t("guard.preview"));
    fileModal?.remove();
    if (!data) return;
    const body = h("div", { class: "file-modal-body" });
    if (data.binary) {
      body.append(h("p", { class: "file-modal-note" }, t("chat.preview.binary")));
    } else if (data.executable) {
      body.append(
        h("p", { class: "file-modal-note" }, t("chat.preview.exe")),
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
          { class: "ghost", title: t("chat.preview.revealHint") },
          t("chat.preview.reveal"),
        ) as HTMLButtonElement
      : h("button", { class: "ghost", title: t("chat.preview.openHint") }, t("chat.preview.open")) as HTMLButtonElement;
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
        h("div", { class: "file-modal-head" }, h("strong", {}, data.path), open, h("button", { class: "ghost", onclick: closeModal }, t("chat.close"))),
        body,
      ),
    );
    // Échap ferme l'aperçu (l'écouteur vit dans la fenêtre, retirée avec elle).
    fileModal.addEventListener("keydown", (event) => {
      if ((event as KeyboardEvent).key === "Escape") {
        event.preventDefault();
        closeModal();
      }
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
    block.append(h("div", { class: "work-kicker" }, t("chat.work.kicker", { count: files.length })));
    for (const file of files) {
      const chip = h("button", { type: "button", class: "work-file", title: t("chat.preview") }, nameOf(file));
      chip.addEventListener("click", () => void openFileModal(file));
      const diff = h("button", { type: "button", class: "ghost small work-diff", title: t("chat.diffHint") }, t("chat.diff"));
      diff.addEventListener("click", () => void showDiff(file));
      block.append(h("div", { class: "work-row" }, chip, diff));
    }
    return block;
  }

  /** Le diff git d'un fichier écrit pendant le tour. Dépôt absent, fichier
   * nouveau non commité ou lenteur : le message reste clair. */
  async function showDiff(file: string) {
    const data = await guard(() => api.fsDiff(file), t("guard.diff"));
    if (!data) return;
    if (!data.diff) {
      toast(data.reason || t("chat.diffUnavailable"));
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
          h("strong", {}, t("chat.diffTitle", { name: nameOf(file) })),
          h("button", { class: "ghost", title: t("chat.previewAsIs"), onclick: () => void openFileModal(file) }, t("chat.previewShort")),
          h("button", { class: "ghost", onclick: closeModal }, t("chat.close")),
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
      logActivity(activityLine(t("chat.act.note"), "note", h("span", { class: "activity-detail" }, announced)));
    }
    if (!pending) {
      pending = h(
        "div",
        { class: "bubble assistant pending" },
        h("p", {}, h("span", { class: "dots" }, h("i"), h("i"), h("i")), t("chat.thinking")),
      );
      append(pending);
    }
  }

  /** Filet : sans signe de vie au bout de 3 minutes, la bulle d'attente
   *  devient une erreur. Réarmé à chaque activité (outil, fragment, étape) :
   *  avant, une tâche longue mais vivante affichait l'erreur quand même.
   *  Suspendu pendant une demande d'autorisation (l'utilisateur peut prendre
   *  son temps), réarmé après sa réponse. */
  function armSafety() {
    window.clearTimeout(safety);
    safetyOn = true;
    safety = window.setTimeout(() => {
      safetyOn = false;
      if (pending || streaming) {
        settle(bubble("error", t("chat.timeout")));
      }
    }, ANSWER_TIMEOUT_MS);
  }

  /** Un événement du tour en cours prouve que Jimy travaille : le filet
   *  repart de zéro. Sans tour suivi (filet jamais armé, ex. commande vocale),
   *  ne rien faire : ne pas étendre le périmètre du filet en douce. */
  function pokeSafety() {
    if (safetyOn && (pending || streaming)) armSafety();
  }

  // Cartes d'autorisation ouvertes, par identifiant de demande.
  const approvalCards = new Map<string, HTMLElement>();
  // Celles d'une tâche de fond : identifiant de demande → tâche.
  const approvalTasks = new Map<string, string>();
  /** Cartes accordées par « Toujours autoriser » (libellé de clôture). */
  const alwaysGiven = new Set<string>();

  /** Clôt les cartes d'un tour : premier plan (`null`) ou tâche de fond. */
  function closeApprovalsOf(taskId: string | null) {
    for (const id of [...approvalCards.keys()]) {
      if ((approvalTasks.get(id) ?? null) === taskId) closeApproval(id, null);
    }
  }

  /** Carte « Autoriser / Refuser » : Jimy veut modifier un fichier sensible
   *  (`.env`, clés, secrets) et son outil est suspendu jusqu'à la réponse. */
  function showApproval(event: AgentEvent) {
    const id = event.id ?? "";
    if (!id || approvalCards.has(id)) return;
    if (event.taskId) approvalTasks.set(id, event.taskId);
    const status = h("span", { class: "approval-status" }, t("chat.approval.waiting"));
    const allow = h("button", { class: "small" }, t("chat.approval.once")) as HTMLButtonElement;
    // « Toujours » : la portée (type d'action ou fichier) est retenue, plus de
    // carte pour elle — elle se retire dans Paramètres → Ajuster les permissions.
    const always = h(
      "button",
      { class: "primary small", title: t("chat.approval.alwaysHint", { scope: event.scope ?? event.target ?? "" }) },
      t("chat.approval.always"),
    ) as HTMLButtonElement;
    const deny = h("button", { class: "danger small" }, t("chat.approval.deny")) as HTMLButtonElement;
    const answer = async (approved: boolean, forever = false) => {
      allow.disabled = true;
      always.disabled = true;
      deny.disabled = true;
      // Marqué avant l'appel : la clôture (`approvalResolved`) peut arriver
      // avant sa réponse.
      if (forever) alwaysGiven.add(id);
      const accepted = await guard(() => api.approvalRespond(id, approved, forever), t("guard.approval"));
      // Demande expirée ou tâche arrêtée entre-temps : rien n'a été exécuté.
      if (accepted === false) closeApproval(id, null);
    };
    allow.addEventListener("click", () => void answer(true));
    always.addEventListener("click", () => void answer(true, true));
    deny.addEventListener("click", () => void answer(false));
    const card = h(
      "div",
      { class: "bubble approval" },
      h(
        "p",
        { class: "approval-title" },
        t(event.taskId ? "chat.approval.titleBg" : "chat.approval.title", {
          what: (event.target ?? "").startsWith("navigateur : ") ? t("chat.approval.browser") : t("chat.approval.file"),
        }),
      ),
      h("code", { class: "approval-target" }, event.target ?? ""),
      h("pre", { class: "approval-detail" }, event.detail ?? ""),
      event.scope ? h("p", { class: "note approval-scope" }, t("chat.approval.scope", { scope: event.scope })) : null,
      h("div", { class: "approval-actions" }, status, deny, allow, always),
    );
    approvalCards.set(id, card);
    clearEmptyState();
    // Au-dessus de la bulle d'attente : elle reste la dernière du fil.
    if (pending?.isConnected) pending.before(card);
    else append(card);
    stream.scrollTop = stream.scrollHeight;
    // Le filet ne concerne que le tour au premier plan.
    if (!event.taskId) {
      window.clearTimeout(safety);
      safetyOn = false;
    }
  }

  /** Clôt une carte : accord, refus, ou `null` = expirée / sans objet. */
  function closeApproval(id: string, approved: boolean | null) {
    const card = approvalCards.get(id);
    if (!card) return;
    approvalCards.delete(id);
    approvalTasks.delete(id);
    card.querySelectorAll("button").forEach((button) => ((button as HTMLButtonElement).disabled = true));
    card.classList.add(approved ? "approved" : "denied");
    const status = card.querySelector(".approval-status");
    const forever = alwaysGiven.delete(id);
    if (status)
      status.textContent =
        approved === null
          ? t("chat.approval.expired")
          : approved
            ? forever
              ? t("chat.approval.allowedForever")
              : t("chat.approval.allowed")
            : t("chat.approval.denied");
  }

  async function submit() {
    const text = input.value.trim();
    if (!text || pending || streaming) return;
    mentionClose();
    turnFiles = [];
    input.value = "";
    autosize();
    const userBubble = bubble("user", text);
    const sentCapture = capture;
    if (sentCapture) userBubble.append(h("div", { class: "capture-note" }, t("chat.capture.attached", { label: sentCapture.label })));
    capture = null;
    renderCapture();
    append(userBubble);
    pending = h(
      "div",
      { class: "bubble assistant pending" },
      h("p", {}, h("span", { class: "dots" }, h("i"), h("i"), h("i")), t("chat.thinking")),
    );
    append(pending);
    setBusy(true);
    armSafety();

    // Premier message d'une conversation neuve : son identifiant n'est connu
    // qu'au retour de `chat`, ses événements peuvent arriver avant.
    const creating = !sessionId;
    if (creating) awaitingNewSession = true;
    const token = navToken;
    const id = await guard(() => api.chat(sessionId, text, sessionId ? null : project, sentCapture?.id ?? null), t("guard.send"));
    if (creating) awaitingNewSession = false;
    // L'utilisateur a changé de conversation pendant l'envoi : il reste où il
    // est, la conversation créée apparaît simplement dans la liste.
    if (token !== navToken) {
      refreshRailSoon();
      return;
    }
    if (!id) {
      settle(bubble("error", t("chat.sendFailed")));
      return;
    }
    sessionId = id;
    ctx.lastSessionId = id;
    // Nouvelle conversation créée, ou la courante remonte en tête.
    refreshRailSoon();
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

  /** Événement d'une tâche de fond : il ne touche ni à la bulle d'attente ni
   *  au filet du tour au premier plan. Sa fin va dans sa conversation si elle
   *  est affichée, sinon un toast l'annonce. */
  function onBackground(wrapper: AgentEvent, event: AgentEvent) {
    const taskId = wrapper.taskId ?? "";
    switch (event.type) {
      case "toolStart": {
        if (event.name === "write_file") {
          const args = typeof event.arguments === "string" ? safeParse(event.arguments) : event.arguments;
          const path = typeof (args as { path?: unknown } | null)?.path === "string" ? String((args as { path: string }).path) : null;
          const files = backgroundFiles.get(taskId) ?? [];
          if (path && !files.includes(path)) files.push(path);
          backgroundFiles.set(taskId, files);
        }
        logActivity(activityLine(t("chat.act.bg"), "tool", h("code", {}, event.name ?? "?")));
        if (backgroundTask?.id === taskId) {
          backgroundTask.step = event.name ?? backgroundTask.step;
          renderTasks();
        }
        break;
      }
      case "progress":
        if (backgroundTask?.id === taskId && event.text) {
          backgroundTask.step = event.text;
          renderTasks();
        }
        break;
      case "approval":
        showApproval({ ...event, taskId });
        break;
      case "approvalResolved":
        closeApproval(event.id ?? "", event.approved ?? false);
        break;
      case "final":
      case "failed": {
        closeApprovalsOf(taskId);
        const title = ctx.taskTitles.get(taskId) ?? "";
        const files = backgroundFiles.get(taskId) ?? [];
        backgroundFiles.delete(taskId);
        if (wrapper.sessionId && wrapper.sessionId === sessionId) {
          const node =
            event.type === "final"
              ? bubble("assistant", event.text ?? "")
              : bubble("error", t("chat.bgFailed", { message: event.message ?? "" }));
          node.classList.add("from-background");
          if (event.type === "final" && files.length > 0) node.append(workBlock(files));
          // Au-dessus d'un tour en cours : sa bulle d'attente reste la dernière.
          const live = pending ?? streaming;
          if (live?.isConnected) live.before(node);
          else append(node);
          stream.scrollTop = stream.scrollHeight;
        } else if (wrapper.sessionId) {
          // Réponse d'une tâche de fond dans une autre conversation : un clic l'ouvre.
          notifyOther(wrapper.sessionId, event.type === "final" ? null : event.message ?? "", title);
        }
        if (wrapper.sessionId === sessionId && sessionId) void markRead(sessionId);
        void refreshTasks();
        refreshRailSoon();
        break;
      }
      default:
        break;
    }
  }

  ctx.onEvent((event: AgentEvent) => {
    // Lot D1 : un événement d'une autre conversation n'entre jamais dans le
    // fil affiché (deux tâches en parallèle, ou conversation changée pendant
    // une réponse). Sa fin est signalée ; elle reste dans sa conversation.
    // La liste des conversations suit tout ce qui change un état : réponse,
    // échec, passage en fond, commande vocale, demande d'accord.
    if (["final", "failed", "detached", "background", "spoken", "approval", "approvalResolved"].includes(event.type)) {
      refreshRailSoon();
    }
    if (fromOtherConversation(event)) {
      if (event.type === "final") notifyOther(event.sessionId as string, null);
      else if (event.type === "failed") notifyOther(event.sessionId as string, event.message ?? "");
      return;
    }
    // Réponse vue dans la conversation affichée : rien de non lu.
    if (event.type === "final" && sessionId) void markRead(sessionId);
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
        logActivity(activityLine(t("chat.act.tool"), "tool", h("code", {}, event.name ?? "?")));
        pokeSafety();
        break;
      case "toolEnd":
        logActivity(
          activityLine(
            event.ok ? t("chat.act.done") : t("chat.act.failed"),
            event.ok ? "ok" : "ko",
            h("code", {}, event.name ?? "?"),
            h("span", { class: "activity-detail" }, event.summary ?? ""),
            event.durationMs ? h("em", {}, `${event.durationMs} ms`) : null,
          ),
        );
        pokeSafety();
        break;
      case "memory":
        logActivity(activityLine(t("chat.act.memory"), "memory", h("span", { class: "activity-detail" }, event.detail ?? "")));
        pokeSafety();
        break;
      case "skill":
        logActivity(activityLine(t("chat.act.skill"), "memory", h("span", { class: "activity-detail" }, event.detail ?? "")));
        pokeSafety();
        break;
      case "progress":
        // Étape d'une longue tâche vocale (aussi dite à voix haute).
        logActivity(activityLine(t("chat.act.step"), "tool", h("span", { class: "activity-detail" }, event.text ?? "")));
        pokeSafety();
        break;
      case "vault":
        logActivity(activityLine(t("chat.act.vault"), "vault", h("span", { class: "activity-detail" }, event.detail ?? "")));
        pokeSafety();
        break;
      case "spoken":
        // Commande vocale : affichée comme un message de l'utilisateur, avec
        // une bulle d'attente en dessous, comme pour un message écrit.
        if (event.sessionId) void adoptVoiceSession(event.sessionId);
        append(bubble("voice", event.text ?? ""));
        // « Arrêter » visible aussi pour une commande vocale.
        setBusy(true);
        if (!pending) {
          pending = h(
            "div",
            { class: "bubble assistant pending" },
            h("p", {}, h("span", { class: "dots" }, h("i"), h("i"), h("i")), t("chat.thinking")),
          );
          append(pending);
        }
        break;
      case "delta": {
        // Fragment reçu en flux : la bulle s'écrit au fil de la génération.
        const target = streamingTarget();
        target.append(event.text ?? "");
        stream.scrollTop = stream.scrollHeight;
        pokeSafety();
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
        closeApprovalsOf(null);
        const node = bubble("assistant", event.text ?? "");
        if (turnFiles.length > 0) node.append(workBlock(turnFiles));
        settle(node);
        break;
      }
      case "failed":
        closeApprovalsOf(null);
        settle(bubble("error", event.message ?? t("chat.failed")));
        break;
      case "detached": {
        // Le tour devient une tâche de fond : le Chat est de nouveau libre.
        // Ce qui s'écrivait en flux n'était qu'une annonce d'étape.
        resolveStreamingAsAnnouncement();
        if (event.taskId) backgroundFiles.set(event.taskId, turnFiles);
        turnFiles = [];
        settle(bubble("assistant", t("chat.detached")));
        void refreshTasks();
        break;
      }
      case "background":
        if (event.event) onBackground(event, event.event);
        break;
      default:
        break;
    }
  });

  function emptyState() {
    const suggest = (label: string, prompt: string) =>
      h(
        "button",
        {
          class: "chat-suggestion",
          type: "button",
          onclick: () => {
            input.value = prompt;
            autosize();
            input.focus();
          },
        },
        label,
      );
    const title = h("h2", { class: "chat-empty-title" }, t("chat.empty.before"));
    title.append(h("em", {}, capitalize(ctx.status.stt.wake_word)), t("chat.empty.after"));
    const chantiersHost = h("div", { class: "home-chantiers-host" });
    mount(
      stream,
      h(
        "div",
        { class: "empty chat-empty" },
        h(
          "div",
          { class: "chat-orb", "aria-hidden": "true" },
          h("div", { class: "chat-orb-ring" }),
          h("div", { class: "chat-orb-ring inner" }),
          h("div", { class: "chat-orb-shell" }, h("div", { class: "chat-orb-core" })),
        ),
        title,
        h("p", { class: "chat-empty-example" }, t("chat.empty.example")),
        h(
          "div",
          { class: "chat-suggestions" },
          suggest(t("chat.suggest.folder"), t("chat.suggest.folderPrompt")),
          suggest(t("chat.suggest.vault"), t("chat.suggest.vaultPrompt")),
          suggest(t("chat.suggest.day"), t("chat.suggest.dayPrompt")),
        ),
        chantiersHost,
      ),
    );
    void api
      .chantiersList(project)
      .then((chantiers) => {
        if (!chantiersHost.isConnected || chantiers.length === 0) return;
        const lead = chantiers.find((c) => c.status === "attente") ?? chantiers[0];
        const resumeLead = () => {
          if (lead.sessionId) void openSession(lead.sessionId);
          else {
            input.value = `${lead.title}${lead.nextStep ? ` — ${lead.nextStep}` : ""}`;
            autosize();
            input.focus();
          }
        };
        const hero = h(
          "section",
          { class: "home-resume-card", "aria-label": t("home.resumeKicker") },
          h(
            "div",
            { class: "home-resume-main" },
            h(
              "span",
              { class: "home-resume-kicker" },
              h("span", { class: "home-resume-dot" }),
              `${t("home.resumeKicker")} · ${lead.status === "attente" ? t("home.waiting") : t("home.open")}`,
            ),
            h("strong", { class: "home-resume-title" }, lead.title),
            lead.nextStep ? h("span", { class: "note" }, t("home.nextStep", { next: lead.nextStep })) : null,
          ),
          h("button", { type: "button", class: "primary home-resume-btn", onclick: resumeLead }, t("home.resumeBtn")),
        );
        const listCard =
          chantiers.length > 1
            ? h(
                "section",
                { class: "home-chantiers-card" },
                h("strong", { class: "home-chantiers-head" }, `${t("home.chantiersTitle")} · ${chantiers.length}`),
                ...chantiers.slice(0, 5).map((c) =>
                  h(
                    "button",
                    {
                      type: "button",
                      class: "home-chantier-row",
                      onclick: () => {
                        if (c.sessionId) void openSession(c.sessionId);
                        else {
                          input.value = `${c.title}${c.nextStep ? ` — ${c.nextStep}` : ""}`;
                          autosize();
                          input.focus();
                        }
                      },
                    },
                    h("span", { class: `home-chantier-dot${c.status === "attente" ? " waiting" : ""}` }),
                    h(
                      "span",
                      { class: "home-chantier-text" },
                      h("strong", {}, c.title),
                      c.nextStep ? h("span", { class: "note" }, c.nextStep) : null,
                    ),
                  ),
                ),
              )
            : null;
        mount(chantiersHost, hero, listCard);
      })
      .catch(() => undefined);
  }

  /**
   * L'événement vient d'une autre conversation que celle affichée ? Ne
   * passent jamais le filtre : les cartes d'autorisation (la tâche attend une
   * réponse), la commande vocale (le Chat adopte sa conversation), les tâches
   * de fond (traitées à part) et l'état global (avatar, écoute). Sans
   * conversation affichée (premier message en cours d'envoi), rien n'est filtré.
   */
  function fromOtherConversation(event: AgentEvent): boolean {
    const global = ["spoken", "approval", "approvalResolved", "background", "detached", "state", "listen", "heard"];
    if (!event.sessionId || global.includes(event.type)) return false;
    if (sessionId) return event.sessionId !== sessionId;
    // Nouvelle conversation vide : tout événement porte une autre conversation,
    // sauf pendant l'envoi du premier message (identifiant pas encore connu).
    return !awaitingNewSession;
  }

  // ── Liste des conversations (étape 1 de la refonte) ─────────────────────
  const rail = conversationRail({
    activeId: () => sessionId,
    // La voix continue la conversation affichée (`conversation_focus`) : le
    // repère n'a de sens que si l'écoute tourne.
    voiceId: () => (ctx.voice?.running ? sessionId : null),
    approvals: () => [...ctx.approvals.values()],
    onOpen: (id) => void openSession(id),
    onNew: (projectPath) => startNewConversation(projectPath),
  });
  ctx.onCleanup(() => rail.dispose());
  let railTimer = 0;
  /** Relit la liste un peu après une rafale d'événements (une seule lecture). */
  function refreshRailSoon() {
    window.clearTimeout(railTimer);
    railTimer = window.setTimeout(() => void rail.refresh(), 400);
  }
  ctx.onCleanup(() => window.clearTimeout(railTimer));

  /** Conversation vue : ses réponses ne comptent plus comme non lues. */
  async function markRead(id: string) {
    await api.sessionMarkRead(id).catch(() => undefined);
    refreshRailSoon();
  }

  /** Réponse (ou échec) arrivée dans une autre conversation : un message
   *  avec « Ouvrir », au lieu d'un renvoi vers les Archives. */
  async function notifyOther(id: string, failure: string | null, knownTitle = "") {
    const title = knownTitle || (await api.sessionGet(id).catch(() => null))?.title || "";
    const open = { label: t("chat.open"), run: () => void openSession(id) };
    if (failure !== null) toast(t("chat.otherFailed", { title: title || "…", message: failure }), "error", open);
    else toast(title ? t("chat.otherReady", { title }) : t("chat.otherReadyUntitled"), "info", open);
  }

  /** Ouvre une conversation sur place : fil, projet, cible de la voix. Le
   *  tour éventuellement en cours ailleurs continue ; ses événements restent
   *  dans sa conversation (lot D1). */
  /** Quitte la conversation affichée : le tour en cours (s'il y en a un)
   *  continue ailleurs, l'affichage repart de zéro. */
  function leaveCurrent() {
    navToken += 1;
    pending?.remove();
    pending = null;
    streaming?.remove();
    streaming = null;
    turnFiles = [];
    setBusy(false);
  }

  async function openSession(id: string) {
    if (id === sessionId) return;
    leaveCurrent();
    sessionId = id;
    ctx.lastSessionId = id;
    void attempt(() => api.conversationFocus(id), t("guard.conversation"));
    mount(activity);
    await loadHistory();
    ctx.approvals.forEach((event) => showApproval(event));
    await loadProject();
    await markRead(id);
    input.focus();
  }

  /** Nouvelle conversation, dans le projet donné (filtre de la liste) ou le
   *  projet courant, comme un nouveau fil chez Codex. */
  function startNewConversation(projectPath: string | null = project) {
    leaveCurrent();
    sessionId = null;
    ctx.lastSessionId = null;
    // La voix ne continue plus l'ancienne conversation.
    void attempt(() => api.conversationFocus(null), t("guard.newConversation"));
    ctx.pendingProject = projectPath;
    if (projectPath !== project) {
      project = projectPath;
      renderProject();
    }
    mount(activity);
    void loadHistory();
    refreshRailSoon();
    input.focus();
  }

  /** Projet de la conversation courante. */
  async function loadProject() {
    const data = await guard(() => api.projectsRecent(), t("guard.projects"));
    if (data) defaultFolder = data.default;
    if (sessionId) {
      // Lu directement : une vieille session (hors des dernières) garde son projet.
      const session = await guard(() => api.sessionGet(sessionId as string), t("guard.session"));
      project = session?.project ?? null;
    }
    renderProject();
  }

  /**
   * Conversation vocale qui démarre ou continue : le Chat l'adopte. Si elle
   * n'a pas de projet et qu'un projet est ouvert dans le Chat, il lui est
   * appliqué — Jimy y travaille aussi à la voix.
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
    const messages = await guard(() => api.sessionMessages(sessionId as string), t("guard.history"));
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
  void refreshTasks();
  // Conversation affichée (Historique, retour sur le Chat) : la voix la
  // continuera. Sans conversation affichée, l'état de la voix est gardé.
  if (sessionId) void attempt(() => api.conversationFocus(sessionId), t("guard.conversation"));
  window.setTimeout(() => input.focus(), 0);

  const newSessionButton = h(
    "button",
    {
      class: "ghost",
      title: t("chat.newHint"),
      // Nouveau fil dans le même projet, comme chez Codex.
      onclick: () => startNewConversation(project),
    },
    t("chat.new"),
  );
  // Conversation affichée à l'ouverture de l'onglet : lue.
  if (sessionId) void markRead(sessionId);

  // Barre du projet : les actions de la conversation y vivent (avant sous la
  // saisie, où elles écrasaient l'aide sur une ligne).
  const chatHeader = h(
    "div",
    { class: "chat-header" },
    h("div", { class: "chat-header-start" }, h("div", { class: "project-picker" }, projectButton, projectMenu), newSessionButton),
    h("div", { class: "chat-header-end" }, captureButton, filesButton),
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
      rail.element,
      h(
        "div",
        { class: "chat-main" },
    stream,
    activity,
    tasksBand,
    h(
      "div",
      { class: "composer" },
      mentionMenu,
      captureChip,
      input,
      h(
        "div",
        { class: "composer-actions" },
        mentionButton,
        h("span", { class: "composer-spacer" }),
        stopButton,
        sendButton,
      ),
    ),
    h("div", { class: "composer-foot" }, hint),
      ),
      explorer.element,
    ),
  );
}
