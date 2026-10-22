// Suite de tests UI de bout en bout, sur la vraie application (CDP).
// Chaque parcours affiche OK / ÉCHEC avec le détail ; code de sortie 1 si un
// parcours échoue.
const { chromium } = require("playwright-core");
const path = require("path");
const OUT = path.resolve(__dirname, "..", "..", "data", "ui-test");
require("fs").mkdirSync(OUT, { recursive: true });

const results = [];
async function step(name, fn) {
  // `UI_ONLY=texte` : ne rejoue que les étapes dont le nom le contient
  // (relance ciblée d'un échec, sans la suite entière).
  if (process.env.UI_ONLY && !name.includes(process.env.UI_ONLY)) return;
  try {
    const detail = await fn();
    results.push(`OK     ${name}${detail ? ` — ${detail}` : ""}`);
  } catch (e) {
    results.push(`ÉCHEC  ${name} — ${e.message.split("\n")[0]}`);
    // Un échec ne doit pas contaminer les parcours suivants : une fenêtre
    // d'aperçu restée ouverte interceptait tous les clics (cascade d'échecs).
    await stepPage?.evaluate(() => document.querySelectorAll(".file-modal").forEach((m) => m.remove())).catch(() => {});
  }
}
let stepPage = null;

(async () => {
  const browser = await chromium.connectOverCDP("http://127.0.0.1:9222");
  const p = browser.contexts()[0].pages().find((pg) => pg.url().includes("tauri.localhost"));
  stepPage = p;
  const errors = [];
  p.on("pageerror", (e) => errors.push(e.message));
  p.on("console", (m) => m.type() === "error" && errors.push(m.text()));
  const nav = async (label) => {
    await p.locator(".nav-item", { hasText: label }).click();
    await p.waitForTimeout(500);
  };
  const expect = (cond, msg) => {
    if (!cond) throw new Error(msg);
  };
  /** Session la plus récente : la liste met les épinglées en tête (lot A,
   *  21 octobre), le premier élément n'est donc plus forcément le dernier. */
  const newest = (list) => [...(list ?? [])].sort((a, b) => (b.updatedAt > a.updatedAt ? 1 : b.updatedAt < a.updatedAt ? -1 : 0))[0];
  /** Fin du tour. Avec le streaming, la bulle d'attente part au premier
   *  fragment, la bulle en flux redevient une bulle d'attente à chaque appel
   *  d'outil, et seule `final` libère le bouton d'envoi : la fin du tour, c'est
   *  ni attente, ni flux, ET bouton libre — dans le même instant. Attendre la
   *  disparition de `.bubble.pending` seule lisait l'historique trop tôt. */
  const attendreReponse = async (timeout = 120_000) => {
    await p.waitForFunction(
      () =>
        !document.querySelector(".bubble.pending, .bubble.streaming") &&
        !document.querySelector(".composer button.primary")?.hasAttribute("disabled"),
      null,
      { timeout, polling: 200 },
    );
  };
  /** Témoin du streaming : une bulle `.streaming` peut naître et disparaître
   *  entre deux sondages (réponse d'un mot) ; un observateur posé AVANT
   *  l'envoi note qu'elle a existé, même brièvement. */
  const guetterFlux = () =>
    p.evaluate(() => {
      window.__fluxVu = false;
      window.__fluxObs?.disconnect();
      window.__fluxObs = new MutationObserver((mutations) => {
        for (const m of mutations) {
          for (const n of m.addedNodes) {
            if (n.nodeType === 1 && n.classList.contains("streaming")) window.__fluxVu = true;
          }
        }
      });
      window.__fluxObs.observe(document.body, { childList: true, subtree: true });
    });
  // L'écoute permanente capte la parole ambiante (musique, vidéo) et répond à
  // des commandes parasites : elles s'ajoutent au fil du Chat et les parcours
  // lisent un fil souillé (« Jimmy a perdu le fil »). Coupée pendant la suite,
  // remise dans son état d'origine à la fin : `voice_stop` enregistre aussi la
  // préférence `listen_on_start`, il faut la rétablir.
  const ecouteInitiale = await p.evaluate(async () => {
    const status = await window.__TAURI_INTERNALS__.invoke("voice_status");
    if (status.running) await window.__TAURI_INTERNALS__.invoke("voice_stop");
    return status.running;
  });
  // Interrupteurs de la Visière (« Micro », « Jimy parle ») : l'utilisateur
  // peut avoir coupé l'écoute ou la synthèse. La suite a besoin des deux ;
  // ils sont relevés ici et rendus tels quels à la fin (piège 61).
  const interrupteurs = await p.evaluate(async () => {
    const s = await window.__TAURI_INTERNALS__.invoke("get_settings");
    const avant = { listen: s.stt.enabled, speech: s.tts.enabled };
    if (!avant.listen) await window.__TAURI_INTERNALS__.invoke("voice_set_enabled", { scope: "listen", enabled: true });
    if (!avant.speech) await window.__TAURI_INTERNALS__.invoke("voice_set_enabled", { scope: "speech", enabled: true });
    return avant;
  });

  await step("Navigation : une seule vue montée par page", async () => {
    for (const label of ["Discuter", "Voix", "Mémoire", "Outils", "Apparence", "Archives", "Réglages", "Diagnostic"]) {
      await nav(label);
      const n = await p.$$eval("main.content > *", (els) => els.length);
      expect(n === 1, `${label} : ${n} vues`);
      const title = await p.locator(".topbar h1").textContent();
      expect(title === label, `titre « ${title} » au lieu de « ${label} »`);
    }
    return "8 vues";
  });

  await step("Fenêtre : barre de titre intégrée (agrandir puis restaurer)", async () => {
    // La barre native est retirée : nos trois boutons doivent la remplacer.
    const labels = await p.$$eval(".titlebar .win-btn", (els) => els.map((e) => e.getAttribute("aria-label")));
    const ok = ["Réduire,Agrandir,Fermer", "Réduire,Restaurer,Fermer"].includes(labels.join(","));
    expect(ok, `boutons : ${labels.join(", ")}`);
    const drag = await p.$(".titlebar[data-tauri-drag-region]");
    expect(drag, "bande de déplacement absente");
    // Agrandir puis restaurer : le libellé suit l'état réel de la fenêtre.
    const btn = p.locator(".titlebar .win-btn").nth(1);
    const avant = await btn.getAttribute("aria-label");
    const apres = avant === "Agrandir" ? "Restaurer" : "Agrandir";
    await btn.click();
    await p.waitForFunction((l) => document.querySelectorAll(".titlebar .win-btn")[1]?.getAttribute("aria-label") === l, apres, { timeout: 5000 });
    await btn.click();
    await p.waitForFunction((l) => document.querySelectorAll(".titlebar .win-btn")[1]?.getAttribute("aria-label") === l, avant, { timeout: 5000 });
    return `${labels.join(", ")} · ${avant} → ${apres} → ${avant}`;
  });

  await step("Chat : envoi, attente visible, réponse", async () => {
    await nav("Discuter");
    await p.locator(".chat-header button", { hasText: "Nouvelle session" }).click();
    await p.locator(".composer-input").fill("Réponds uniquement par le mot : banane");
    await guetterFlux();
    await p.keyboard.press("Enter");
    await p.waitForSelector(".bubble.pending", { timeout: 5000 });
    const busy = await p.locator(".composer button.primary").textContent();
    expect(busy.includes("travaille"), `bouton pendant l'attente : « ${busy} »`);
    await attendreReponse(150_000);
    // Streaming : la réponse a dû passer par une bulle qui s'écrit au fil.
    const fluxVu = await p.evaluate(() => window.__fluxVu === true);
    expect(fluxVu, "aucune bulle en flux pendant la génération : la réponse n'est plus streamée");
    const last = await p.locator(".stream .bubble").last();
    const cls = await last.getAttribute("class");
    const text = (await last.textContent()).trim();
    expect(cls.includes("assistant"), `dernière bulle : ${cls} « ${text} »`);
    const free = await p.locator(".composer button.primary").textContent();
    expect(free === "Envoyer", `bouton après réponse : « ${free} »`);
    const labels = await p.$$eval(".stream .bubble", (els) => els.map((e) => e.textContent));
    expect(!labels.some((t) => /^(Jimy|Jimmy|Vous)/.test(t)), "libellé en double dans une bulle");
    return `« ${text.slice(0, 60)} »`;
  });

  await step("Chat : la pastille revient à « prêt » après la réponse", async () => {
    await p.waitForFunction(() => document.querySelector(".topbar .visor-text")?.textContent === "prêt", null, { timeout: 15000 });
    return "prêt";
  });

  await step("Chat : la session est reprise après navigation", async () => {
    const before = await p.locator(".stream .bubble").count();
    await nav("Mémoire");
    await nav("Discuter");
    await p.waitForTimeout(800);
    const after = await p.locator(".stream .bubble").count();
    expect(after === before, `${before} bulles avant, ${after} après`);
    return `${after} bulles`;
  });

  // Cas réel du 4 octobre : chaque message ouvrait une session neuve et Jimmy
  // perdait le fil (« installe-le » → « installer quoi ? »).
  await step("Chat : le second message garde le fil (même session)", async () => {
    // `sessions` est plafonné à 50 : on compare la session la plus récente,
    // pas leur nombre. Une session neuve passerait en tête de liste.
    const latest = async () => newest(await p.evaluate(() => window.__TAURI_INTERNALS__.invoke("sessions")))?.id;
    const sessionBefore = await latest();
    await p.locator(".composer-input").fill("Quel mot viens-tu d'écrire juste avant ? Réponds en un seul mot.");
    await p.keyboard.press("Enter");
    await p.waitForSelector(".bubble.pending", { timeout: 5000 });
    await attendreReponse(120_000);
    const sessionAfter = await latest();
    expect(sessionAfter === sessionBefore, `une session neuve a été créée (${sessionBefore} → ${sessionAfter})`);
    // La réponse enregistrée dans CETTE session, pas la dernière bulle : une
    // commande vocale captée pendant le test (écoute active, faux « Jimmy »)
    // s'affiche aussi dans le flux et faussait la lecture (4 octobre).
    const messages = await p.evaluate(
      (id) => window.__TAURI_INTERNALS__.invoke("session_messages", { sessionId: id }),
      sessionAfter,
    );
    const answers = messages.filter((m) => m.role === "assistant" && m.content.trim());
    const text = (answers[answers.length - 1]?.content ?? "").trim();
    expect(answers.length >= 2, `${answers.length} réponse(s) dans la session (attendu 2)`);
    expect(/banane/i.test(text), `Jimmy a perdu le fil : « ${text} »`);
    return `« ${text.slice(0, 40)} », même session`;
  });

  await step("Chat : projet de la conversation et explorateur de fichiers", async () => {
    const invoke = (cmd, args) => p.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args ?? {}]);
    const repo = (await invoke("paths_info")).app;
    const repoName = repo.split(/[\\/]/).filter(Boolean).pop();
    const session = newest(await invoke("sessions"));
    // Le sélecteur de dossier natif ne se pilote pas : on rattache le projet
    // par la commande qu'il appelle, puis on vérifie tout le reste à l'écran.
    await invoke("session_set_project", { sessionId: session.id, project: repo });
    await nav("Mémoire");
    await nav("Discuter");
    await p.waitForFunction((name) => document.querySelector(".project-name")?.textContent === name, repoName, { timeout: 5000 });
    // Le menu des projets propose d'ouvrir un dossier, puis se ferme.
    await p.locator(".project-button").click();
    await p.waitForSelector(".project-menu:not([hidden]) .project-item.open", { timeout: 5000 });
    // Clic hors du menu (le haut du fil est sous le menu ouvert, qui intercepterait le clic).
    await p.locator(".composer-hint").click();
    expect(await p.locator(".project-menu").isHidden(), "le menu des projets reste ouvert");
    // Explorateur : arborescence du projet, dossier déplié, aperçu d'un fichier.
    if (await p.locator(".explorer").isHidden()) await p.locator(".files-button").click();
    await p.waitForSelector(".explorer .tree-row", { timeout: 5000 });
    const names = await p.$$eval(".explorer .tree-name", (els) => els.map((e) => e.textContent));
    expect(names.includes("agent") && names.includes("README.md"), `arborescence : ${names.slice(0, 10).join(", ")}`);
    expect(!names.includes(".git"), "le dossier .git ne doit pas apparaître");
    await p.locator(".explorer .tree-row.dir", { has: p.locator(".tree-name", { hasText: /^agent$/ }) }).click();
    await p.waitForFunction(() => [...document.querySelectorAll(".explorer .tree-name")].some((e) => e.textContent === "src"), null, { timeout: 5000 });
    await p.locator(".explorer .tree-row", { has: p.locator(".tree-name", { hasText: /^README\.md$/ }) }).first().click();
    await p.waitForSelector(".explorer-preview:not([hidden]) .preview-text", { timeout: 5000 });
    const preview = (await p.locator(".preview-text").textContent()) ?? "";
    expect(/Jimmy/i.test(preview), "aperçu du README vide ou faux");
    // L'agent travaille bien dans le projet de la conversation.
    await p.locator(".composer-input").fill("Quel est ton dossier de travail pour cette conversation ? Réponds uniquement par le chemin complet, sans outil.");
    await p.keyboard.press("Enter");
    await p.waitForSelector(".bubble.pending", { timeout: 5000 });
    await attendreReponse(120_000);
    const messages = await invoke("session_messages", { sessionId: session.id });
    const answer = messages.filter((m) => m.role === "assistant").pop()?.content ?? "";
    expect(answer.toLowerCase().includes(repoName.toLowerCase()), `dossier de travail annoncé : « ${answer} »`);
    return `projet ${repoName}, ${names.length} entrées, aperçu et dossier de travail OK`;
  });

  // Mention « @ » (façon Codex) : @ + lettres → menu des fichiers du projet,
  // Entrée insère le chemin relatif SANS envoyer le message.
  await step("Chat : « @ » cite un fichier du projet", async () => {
    const input = p.locator(".composer-input");
    await input.fill("@READ");
    await p.waitForSelector(".mention-menu:not([hidden]) .mention-item", { timeout: 5000 });
    const items = await p.$$eval(".mention-menu .mention-name", (els) => els.map((e) => e.textContent));
    // Tri par pertinence : le README de la racine passe devant les README
    // enfouis (profil navigateur dans data/).
    expect(items[0] === "README.md", `menu « @ » : ${items.join(", ")}`);
    await p.keyboard.press("ArrowDown"); // navigation : item suivant…
    await p.keyboard.press("ArrowUp"); // …puis retour au premier
    await p.keyboard.press("Enter");
    const value = await input.inputValue();
    expect(value.startsWith("@README.md "), `inséré après Entrée : « ${value} »`);
    expect((await p.locator(".bubble.pending").count()) === 0, "Entrée a soumis au lieu d'insérer");
    // Échap ferme le menu sans agir ; la composition continue.
    await input.press("Escape");
    expect(await p.locator(".mention-menu").isHidden(), "Échap n'a pas fermé le menu");
    await input.fill("");
    return `menu « @ » : ${items.length} item(s), insertion sans envoi`;
  });

  // Chemin cliquable (façon Codex) : un chemin absolu dans une réponse devient
  // une puce ; son clic ouvre l'aperçu (avec saut de ligne quand « :n »).
  await step("Chat : un chemin dans la réponse s'ouvre en aperçu", async () => {
    const target = `${path.resolve(__dirname, "..", "..", "README.md")}:3`;
    const input = p.locator(".composer-input");
    // L'attente après l'envoi : une seule demande, une réponse pilote.
    await input.fill(`Réponds UNIQUEMENT par ce chemin, copié exactement, sans phrase : ${target}`);
    await p.keyboard.press("Enter");
    await p.waitForSelector(".bubble.pending", { timeout: 5000 });
    await attendreReponse(180_000);
    await p.waitForSelector(".bubble.assistant .msg-path", { timeout: 5000 });
    const label = (await p.locator(".bubble.assistant .msg-path").textContent()) ?? "";
    expect(label.replace(/\D/g, "").includes("3"), `étiquette de la puce : « ${label} »`);
    await p.locator(".bubble.assistant .msg-path").first().click();
    await p.waitForSelector(".file-modal-card", { timeout: 5000 });
    const head = (await p.locator(".file-modal-head strong").textContent()) ?? "";
    expect(head.includes("README.md"), `aperçu ouvert dans : « ${head} »`);
    await p.waitForSelector(".file-modal-line.target", { timeout: 5000 });
    await p.keyboard.press("Escape");
    await p.waitForSelector(".file-modal", { state: "detached", timeout: 5000 });
    return `puce « ${label} », aperçu + ligne surlignée`;
  });

  // Bloc « travaux » (façon Codex) : les fichiers écrits par Jimmy dans le
  // tour apparaissent sous la réponse, en puces cliquables. Ici data/tests-ui/
  // (dossier de données local, jamais commité) : le diff silencieux est
  // acceptable, seules les puces sont vérifiées.
  await step("Chat : le bloc « travaux » liste les fichiers écrits", async () => {
    const note = (await p.evaluate(() => window.__TAURI_INTERNALS__.invoke("paths_info"))).data;
    const fichier = `${note}\\tests-ui\\reussite-${Date.now() % 10_000}.txt`;
    const input = p.locator(".composer-input");
    await input.fill(`Écris dans « ${fichier} » la ligne unique : test travaux. Ne renvoie que la confirmation en une phrase.`);
    await p.keyboard.press("Enter");
    await p.waitForSelector(".bubble.pending", { timeout: 5000 });
    await attendreReponse(180_000);
    await p.waitForSelector(".bubble.assistant .work-block .work-file", { timeout: 10000 });
    const chip = (await p.locator(".work-file").textContent()) ?? "";
    expect(chip.endsWith(".txt"), `puce du bloc travaux : « ${chip} »`);
    return `bloc travaux visible : « ${chip} »`;
  });

  // Cas réel du 5 octobre : un `.env` de production réécrit sans rien
  // demander. Modifier un fichier sensible doit afficher une carte
  // d'autorisation ; « Refuser » = le fichier n'est pas touché.
  await step("Chat : modifier un fichier sensible demande l'autorisation", async () => {
    const note = (await p.evaluate(() => window.__TAURI_INTERNALS__.invoke("paths_info"))).data;
    const fichier = `${note}\\tests-ui\\garde-${Date.now() % 10_000}.env`;
    await p.locator(".composer-input").fill(`Écris dans « ${fichier} » la ligne unique : TEST=1. Utilise write_file.`);
    await p.keyboard.press("Enter");
    await p.waitForSelector(".bubble.approval", { timeout: 150_000 });
    const target = (await p.locator(".bubble.approval .approval-target").last().textContent()) ?? "";
    expect(target.endsWith(".env"), `fichier annoncé sur la carte : « ${target} »`);
    // Trois réponses possibles depuis le 10 octobre.
    const boutons = await p.locator(".bubble.approval").last().locator("button").allTextContents();
    expect(
      ["Refuser", "Autoriser une fois", "Toujours autoriser"].every((b) => boutons.includes(b)),
      `boutons de la carte : ${boutons.join(", ")}`,
    );
    await p.locator(".bubble.approval button", { hasText: "Refuser" }).last().click();
    await p.waitForSelector(".bubble.approval.denied", { timeout: 10_000 });
    await attendreReponse(180_000);
    const existe = await p.evaluate(
      (f) => window.__TAURI_INTERNALS__.invoke("fs_preview", { path: f }).then(() => true, () => false),
      fichier,
    );
    expect(!existe, `le fichier a été écrit malgré le refus : ${fichier}`);
    return `carte « ${target.split("\\").pop()} », refus respecté`;
  });

  // « Toujours autoriser » : la même portée (ici un fichier) passe ensuite
  // sans carte ; retirée dans Paramètres → permissions, elle disparaît.
  await step("Chat : « Toujours autoriser » ne redemande plus, retrait dans Paramètres", async () => {
    const invoke = (cmd, args) => p.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args ?? {}]);
    const note = (await invoke("paths_info")).data;
    const fichier = `${note}\\tests-ui\\toujours-${Date.now() % 10_000}.env`;
    const avant = (await p.locator(".bubble.approval").count());
    await p.locator(".composer-input").fill(`Écris dans « ${fichier} » la ligne unique : TEST=1. Utilise write_file.`);
    await p.keyboard.press("Enter");
    await p.waitForFunction((n) => document.querySelectorAll(".bubble.approval").length > n, avant, { timeout: 150_000 });
    await p.locator(".bubble.approval button", { hasText: "Toujours autoriser" }).last().click();
    await p.waitForSelector(".bubble.approval.approved", { timeout: 10_000 });
    await attendreReponse(180_000);
    const portees = await invoke("approvals_always");
    const portee = portees.find((k) => k.endsWith(fichier.split("\\").pop().toLowerCase()));
    expect(portee, `accord non retenu : ${JSON.stringify(portees)}`);
    // Seconde écriture du même fichier : aucune nouvelle carte.
    const cartes = await p.locator(".bubble.approval").count();
    await p.locator(".composer-input").fill(`Réécris « ${fichier} » avec la ligne unique : TEST=2. Utilise write_file.`);
    await p.keyboard.press("Enter");
    await attendreReponse(180_000);
    expect((await p.locator(".bubble.approval").count()) === cartes, "une carte est revenue malgré « Toujours »");
    const contenu = await invoke("fs_preview", { path: fichier }).then((r) => JSON.stringify(r), () => "");
    expect(contenu.includes("TEST=2"), `seconde écriture absente : ${contenu.slice(0, 120)}`);
    // Retrait depuis Paramètres → Ajuster les permissions.
    await nav("Réglages");
    await p.locator("button", { hasText: "Ajuster les permissions" }).click();
    const ligne = p.locator(".always-row", { hasText: portee });
    await ligne.locator("button", { hasText: "Retirer" }).click();
    await p.waitForFunction((k) => ![...document.querySelectorAll(".always-row code")].some((c) => c.textContent === k), portee, {
      timeout: 5000,
    });
    expect(!(await invoke("approvals_always")).includes(portee), "accord toujours présent après retrait");
    await nav("Discuter");
    return `« ${portee.split("\\").pop()} » : accordé, réécrit sans carte, retiré`;
  });

  // Cas réel du 4 octobre (piège 69) : un projet choisi hors d'une
  // conversation enregistrée se perdait au changement d'onglet.
  await step("Chat : le projet d'une nouvelle conversation survit à la navigation", async () => {
    const repo = (await p.evaluate(() => window.__TAURI_INTERNALS__.invoke("paths_info"))).app;
    const repoName = repo.split(/[\\/]/).filter(Boolean).pop();
    const nameIs = (n) => p.waitForFunction((x) => document.querySelector(".project-name")?.textContent === x, n, { timeout: 5000 });
    // Nouvelle conversation : elle garde le projet en cours.
    await p.locator(".chat-header button", { hasText: "Nouvelle session" }).click();
    await nameIs(repoName);
    // Retour au dossier par défaut, puis le projet récent, sans envoyer de message.
    await p.locator(".project-button").click();
    await p.waitForSelector(".project-menu:not([hidden])", { timeout: 5000 });
    await p.locator(".project-menu .project-item", { hasText: "Dossier par défaut" }).click();
    await nameIs("aucun (dossier par défaut)");
    await p.locator(".project-button").click();
    await p.waitForSelector(".project-menu:not([hidden])", { timeout: 5000 });
    await p.locator(".project-menu .project-item", { hasText: repoName }).first().click();
    await nameIs(repoName);
    await nav("Mémoire");
    await nav("Discuter");
    await nameIs(repoName);
    return `« ${repoName} » gardé après navigation`;
  });

  // Arrêt d'urgence depuis le Chat (équivalent du « STOP » vocal).
  await step("Chat : le bouton Arrêter interrompt la tâche en cours", async () => {
    const invoke = (cmd, args) => p.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args ?? {}]);
    await p.locator(".composer-input").fill("Lis un par un tous les fichiers du dossier agent et résume chacun en détail.");
    await p.keyboard.press("Enter");
    await p.waitForSelector(".stop-button:not([hidden])", { timeout: 5000 });
    await p.waitForTimeout(2500);
    const t0 = Date.now();
    await p.locator(".stop-button").click();
    await attendreReponse(15_000);
    const ms = Date.now() - t0;
    const text = ((await p.locator(".stream .bubble.assistant").last().textContent()) ?? "").trim();
    expect(/Arrêté/.test(text), `réponse après l'arrêt : « ${text} »`);
    expect(await p.locator(".stop-button").isHidden(), "le bouton Arrêter reste visible");
    // La conversation de test ne reste pas dans l'historique de l'utilisateur.
    const latest = newest(await invoke("sessions"));
    if (latest?.title?.startsWith("Lis un par un")) await invoke("delete_session", { sessionId: latest.id });
    return `arrêté en ${ms} ms`;
  });

  // Tâches de fond (7 octobre) : une tâche outillée qui dure passe en
  // arrière-plan, le Chat redevient libre, une question posée pendant ce
  // temps a sa réponse, et la fin de la tâche arrive dans le fil.
  await step("Chat : une tâche longue passe en arrière-plan, le Chat reste libre", async () => {
    const invoke = (cmd, args) => p.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args ?? {}]);
    // Une tâche d'un parcours précédent peut être passée en fond (le refus du
    // fichier sensible, si le modèle est lent) : une seule place de fond, on
    // attend qu'elle se libère pour ne pas lire son bandeau à elle.
    for (let i = 0; i < 180 && (await invoke("tasks_list")).some((t) => t.background); i++) await p.waitForTimeout(1000);
    await p.locator(".chat-header button", { hasText: "Nouvelle session" }).click();
    await p.locator(".composer-input").fill(
      "Exécute avec run_command la commande : Start-Sleep -Seconds 70; 'fini-fond'. Puis réponds uniquement par ce qu'elle affiche.",
    );
    await p.keyboard.press("Enter");
    const t0 = Date.now();
    // Passage en fond : 45 s après le premier outil (`tasks::DETACH_AFTER`),
    // et c'est bien cette tâche-ci qu'annonce le bandeau.
    await p.waitForFunction(
      () => document.querySelector(".tasks-band:not([hidden]) .tasks-band-title")?.textContent?.startsWith("Exécute avec run_command"),
      null,
      { timeout: 120_000, polling: 200 },
    );
    const detachedAfter = Math.round((Date.now() - t0) / 1000);
    await attendreReponse(10_000);
    const free = await p.locator(".composer button.primary").textContent();
    expect(free === "Envoyer", `bouton pendant la tâche de fond : « ${free} »`);
    const tasks = await invoke("tasks_list");
    expect(tasks.some((t) => t.background), "aucune tâche de fond dans le registre");
    // Question en parallèle : réponse au premier plan pendant la tâche.
    await p.locator(".composer-input").fill("Réponds uniquement par le mot : cerise");
    await p.keyboard.press("Enter");
    await p.waitForSelector(".bubble.pending", { timeout: 5000 });
    await attendreReponse(120_000);
    const parallel = (await p.locator(".stream .bubble.assistant:not(.from-background)").last().textContent()) ?? "";
    expect(/cerise/i.test(parallel), `réponse en parallèle : « ${parallel.trim()} »`);
    // Fin de la tâche de fond, dans sa conversation, et bandeau retiré.
    await p.waitForSelector(".bubble.from-background", { timeout: 180_000 });
    await p.waitForSelector(".tasks-band[hidden]", { state: "attached", timeout: 10_000 });
    const fin = ((await p.locator(".bubble.from-background").last().textContent()) ?? "").trim();
    const session = newest(await invoke("sessions"));
    if (session?.title?.startsWith("Exécute avec run_command")) await invoke("delete_session", { sessionId: session.id });
    return `fond après ${detachedAfter} s, « ${parallel.trim()} » en parallèle, fin : « ${fin.slice(0, 40)} »`;
  });

  // Arrêt d'une tâche de fond (12 octobre) : le bouton du bandeau l'arrête et
  // le bandeau disparaît aussitôt (avant, il restait affiché si la fin de la
  // tâche n'arrivait pas par événement : clics « sans effet »).
  await step("Chat : « Arrêter » du bandeau arrête la tâche de fond et retire le bandeau", async () => {
    const invoke = (cmd, args) => p.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args ?? {}]);
    for (let i = 0; i < 180 && (await invoke("tasks_list")).some((t) => t.background); i++) await p.waitForTimeout(1000);
    await p.locator(".chat-header button", { hasText: "Nouvelle session" }).click();
    await p.locator(".composer-input").fill(
      "Exécute avec run_command la commande : Start-Sleep -Seconds 120; 'fond-stop'. Puis réponds uniquement par ce qu'elle affiche.",
    );
    await p.keyboard.press("Enter");
    await p.waitForFunction(
      () => document.querySelector(".tasks-band:not([hidden]) .tasks-band-title")?.textContent?.includes("fond-stop"),
      null,
      { timeout: 120_000, polling: 200 },
    );
    const t0 = Date.now();
    await p.locator(".tasks-band-stop").click();
    await p.waitForSelector(".tasks-band[hidden]", { state: "attached", timeout: 6000 });
    const ms = Date.now() - t0;
    const tasks = await invoke("tasks_list");
    expect(!tasks.some((t) => t.background), "la tâche de fond tourne encore");
    const session = newest(await invoke("sessions"));
    if (session?.title?.startsWith("Exécute avec run_command")) await invoke("delete_session", { sessionId: session.id });
    return `bandeau retiré ${ms} ms après le clic`;
  });

  // Arrêt d'urgence global (12 octobre) : bouton de la barre du haut visible
  // pendant le travail, Échap l'actionne depuis n'importe où dans la fenêtre.
  await step("Arrêt d'urgence : bouton de la barre du haut et touche Échap", async () => {
    const invoke = (cmd, args) => p.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args ?? {}]);
    await p.locator(".chat-header button", { hasText: "Nouvelle session" }).click();
    await p.locator(".composer-input").fill("Lis un par un tous les fichiers du dossier agent et résume chacun en détail.");
    await p.keyboard.press("Enter");
    await p.waitForSelector(".global-stop:not([hidden])", { timeout: 15_000 });
    await p.waitForTimeout(1500);
    const t0 = Date.now();
    // Échap hors de la zone de saisie : depuis n'importe où dans la fenêtre.
    await p.evaluate(() => (document.activeElement instanceof HTMLElement ? document.activeElement.blur() : undefined));
    await p.keyboard.press("Escape");
    await attendreReponse(15_000);
    const ms = Date.now() - t0;
    const text = ((await p.locator(".stream .bubble.assistant").last().textContent()) ?? "").trim();
    expect(/Arrêté/.test(text), `réponse après Échap : « ${text} »`);
    const latest = newest(await invoke("sessions"));
    if (latest?.title?.startsWith("Lis un par un")) await invoke("delete_session", { sessionId: latest.id });
    return `arrêté par Échap en ${ms} ms`;
  });

  // Vision (7 octobre) : le bouton capture la fenêtre active (hors Jimmy),
  // la vignette s'affiche et se retire. Rien n'est envoyé : la suite ne doit
  // pas transmettre l'écran réel au fournisseur (lecture d'image prouvée par
  // `--test protocols chaque_format_lit_une_image`).
  await step("Chat : « Joindre ma fenêtre » capture et se retire", async () => {
    await p.locator(".capture-button").click();
    await p.waitForSelector(".capture-chip:not([hidden]) .capture-thumb", { timeout: 15_000 });
    const label = ((await p.locator(".capture-label").textContent()) ?? "").trim();
    expect(/^Fenêtre jointe : « .+ »$/.test(label), `libellé de la capture : « ${label} »`);
    const ok = await p.evaluate(() => {
      const img = document.querySelector(".capture-thumb");
      return img instanceof HTMLImageElement && img.src.startsWith("data:image/jpeg;base64,");
    });
    expect(ok, "vignette absente ou pas en JPEG");
    // La fenêtre principale s'intitule « Jimy » : elle doit être écartée.
    expect(label !== "Fenêtre jointe : « Jimy »", "la capture vise la fenêtre de Jimy");
    await p.locator(".capture-chip button", { hasText: "Retirer" }).click();
    await p.waitForSelector(".capture-chip[hidden]", { state: "attached", timeout: 5000 });
    return label;
  });

  await step("Voix : activer, état conservé entre vues, couper", async () => {
    await nav("Voix");
    let state = await p.locator(".listen-state").textContent();
    if (state === "active") {
      await p.locator("button", { hasText: "Couper l'écoute" }).click();
      await p.waitForFunction(() => document.querySelector(".listen-state")?.textContent === "arrêtée", null, { timeout: 15000 });
    }
    await p.locator("button", { hasText: "Activer l'écoute" }).click();
    await p.waitForFunction(() => document.querySelector(".listen-state")?.textContent === "active", null, { timeout: 90000 });
    const servers = await p.locator(".card", { hasText: "Écoute permanente" }).locator(".note").nth(1).textContent();
    expect(servers.includes("prêt"), `serveurs : ${servers}`);
    await nav("Discuter");
    const side = await p.locator(".status-line", { hasText: "Écoute" }).textContent();
    expect(side.includes("active"), `barre latérale : ${side}`);
    await nav("Voix");
    await p.waitForTimeout(800);
    state = await p.locator(".listen-state").textContent();
    expect(state === "active", `après retour sur Voix : ${state}`);
    // Un second démarrage ne doit pas lancer une deuxième boucle (idempotent).
    const meterVisible = await p.locator(".card", { hasText: "Écoute permanente" }).locator(".meter").isVisible();
    expect(meterVisible, "vumètre de l'écoute invisible");
    await p.locator("button", { hasText: "Couper l'écoute" }).click();
    await p.waitForFunction(() => document.querySelector(".listen-state")?.textContent === "arrêtée", null, { timeout: 15000 });
    return servers.replace(/\s+/g, " ").trim();
  });

  await step("Voix : bandeau de phase et état d'attente visibles", async () => {
    await nav("Voix");
    // Le bandeau n'existe visiblement que pendant l'écoute : on l'active, on
    // vérifie, puis on recoupe pour laisser l'état attendu par l'étape suivante.
    await p.locator("button", { hasText: "Activer l'écoute" }).click();
    await p.waitForFunction(() => document.querySelector(".listen-state")?.textContent === "active", null, { timeout: 90000 });
    await p.waitForSelector(".phase-banner", { state: "visible", timeout: 5000 });
    const text = (await p.locator(".phase-banner .phase-text").textContent()) ?? "";
    expect(/veille|t'entends|transcris|À toi|réfléchis|réponds/.test(text), `bandeau : « ${text} »`);
    const chip = (await p.locator(".topbar .visor-text").textContent()) ?? "";
    await p.locator("button", { hasText: "Couper l'écoute" }).click();
    await p.waitForFunction(() => document.querySelector(".listen-state")?.textContent === "arrêtée", null, { timeout: 15000 });
    const hidden = await p.locator(".phase-banner").isHidden();
    expect(hidden, "le bandeau doit disparaître quand l'écoute est coupée");
    return `bandeau « ${text} », pastille « ${chip} »`;
  });

  await step("Voix : relance de l'écoute (serveurs whisper conservés)", async () => {
    await p.locator("button", { hasText: "Activer l'écoute" }).click();
    await p.waitForFunction(() => document.querySelector(".listen-state")?.textContent === "active", null, { timeout: 90000 });
    await p.waitForTimeout(1500);
    const servers = await p.locator(".card", { hasText: "Écoute permanente" }).locator(".note").nth(1).textContent();
    expect(!servers.includes("indisponible"), `après relance : ${servers}`);
    return "prêt après relance";
  });

  // Skin actif de l'utilisateur au départ : restauré à la fin des parcours
  // Skin (INSTRUCTIONS §6 : un test rend le réglage qu'il a trouvé).
  let initialSkin = "";
  await step("Skin : changement appliqué et bouton actif mis à jour", async () => {
    await nav("Apparence");
    initialSkin = await p.$$eval(".card button.primary", (els) => {
      const skin = els.find((b) => b.closest(".card")?.textContent.includes("Skin"));
      return skin ? skin.textContent.trim() : "";
    });
    // Le changement de skin relance Godot : sa durée varie (1,5 s fixes ont
    // échoué le 6 octobre). On attend l'état du bouton, 15 s au plus.
    const actif = async (label) => {
      await p
        .waitForFunction(
          (text) => [...document.querySelectorAll("button")].some((b) => b.textContent.includes(text) && b.className === "primary"),
          label,
          { timeout: 15000, polling: 200 },
        )
        .catch(() => {});
      return p.locator("button", { hasText: label }).getAttribute("class");
    };
    await p.locator("button", { hasText: "Fennec" }).click();
    const cls = await actif("Fennec");
    expect(cls === "primary", `bouton Fennec : ${cls}`);
    await p.locator("button", { hasText: "Renard roux" }).click();
    const back = await actif("Renard roux");
    expect(back === "primary", `retour renard : ${back}`);
    const dodge = p.locator(".card", { hasText: "Comportement" }).locator("input[type=checkbox]");
    expect((await dodge.count()) === 1, "interrupteur d'esquive absent");
    const states = await p.$$eval(".state-chip", (els) => els.map((e) => e.textContent));
    expect(states.includes("j'écoute"), `états non traduits : ${states.join(", ")}`);
    return "fennec puis renard";
  });

  await step("Skin : skins premium (licence refusée, débloqué = applicable)", async () => {
    await nav("Apparence");
    const premium = p.locator(".card", { hasText: "Skins premium" });
    expect((await premium.count()) === 1, "carte « Skins premium » absente");
    const neon = premium.locator("button", { hasText: "Renard Néon" });
    expect((await neon.count()) === 1, "bouton Renard Néon absent");
    const locked = await neon.isDisabled();
    if (locked) {
      // Machine sans licence : une fausse clé est refusée, rien n'est débloqué.
      await premium.locator("input").fill("JIMY-faux.faux");
      await premium.locator("button", { hasText: "Activer" }).click();
      const toastText = await p.waitForSelector(".toast.error", { timeout: 5000 }).then((t) => t.textContent());
      expect(toastText.includes("invalide"), `toast : ${toastText}`);
      expect(await neon.isDisabled(), "Néon débloqué par une fausse clé");
      return "verrouillé, fausse clé refusée";
    }
    // Machine avec licence : le Néon s'applique, puis retour au renard.
    await neon.click();
    await p
      .waitForFunction(
        () => [...document.querySelectorAll("button")].some((b) => b.textContent.includes("Renard Néon") && b.className === "primary"),
        null,
        { timeout: 15000, polling: 200 },
      )
      .catch(() => {});
    const cls = await p.locator("button", { hasText: "Renard Néon" }).getAttribute("class");
    expect(cls === "primary", `bouton Néon : ${cls}`);
    await p.locator("button", { hasText: "Renard roux" }).click();
    await p
      .waitForFunction(
        () => [...document.querySelectorAll("button")].some((b) => b.textContent.includes("Renard roux") && b.className === "primary"),
        null,
        { timeout: 15000, polling: 200 },
      )
      .catch(() => {});
    const back = await p.locator("button", { hasText: "Renard roux" }).getAttribute("class");
    expect(back === "primary", `retour renard : ${back}`);
    return "débloqué : néon puis renard";
  });

  // ARK : modèle 3D à squelette natif (.glb). Godot refuse un skin dont le
  // modèle manque : un bouton « primary » prouve que le .glb est chargé.
  await step("Skin : ARK (modèle 3D) appliqué puis renard", async () => {
    await nav("Apparence");
    const ark = p.locator(".card", { hasText: "Skins premium" }).locator("button", { hasText: "ARK" });
    expect((await ark.count()) === 1, "bouton ARK absent");
    if (await ark.isDisabled()) return "verrouillé (machine sans licence)";
    const primary = (label) =>
      p
        .waitForFunction(
          (l) => [...document.querySelectorAll("button")].some((b) => b.textContent.trim() === l && b.className === "primary"),
          label,
          { timeout: 20000, polling: 200 },
        )
        .catch(() => {});
    await ark.click();
    await primary("ARK");
    expect((await ark.getAttribute("class")) === "primary", "ARK non appliqué");
    await p.locator("button", { hasText: "Renard roux" }).click();
    await primary("Renard roux");
    const back = await p.locator("button", { hasText: "Renard roux" }).getAttribute("class");
    expect(back === "primary", `retour renard : ${back}`);
    return "ARK puis renard";
  });

  await step("Skin : skin de l'utilisateur restauré", async () => {
    await nav("Apparence");
    if (!initialSkin || initialSkin === "Renard roux") return "renard (rien à restaurer)";
    await p.locator(".card button", { hasText: initialSkin }).first().click();
    await p
      .waitForFunction(
        (label) => [...document.querySelectorAll(".card button")].some((b) => b.textContent.trim() === label && b.className === "primary"),
        initialSkin,
        { timeout: 15000, polling: 200 },
      )
      .catch(() => {});
    const cls = await p.locator(".card button", { hasText: initialSkin }).first().getAttribute("class");
    expect(cls === "primary", `restauration de « ${initialSkin} » : ${cls}`);
    return `« ${initialSkin} » restauré`;
  });

  await step("Paramètres : enregistrement confirmé", async () => {
    await nav("Réglages");
    await p.waitForTimeout(800);
    // La langue et l'écoute ont déménagé dans l'onglet Voix : on vérifie un
    // champ resté ici avant d'enregistrer.
    const workspace = await p.locator(".field-row", { hasText: "Dossier de travail" }).locator("input").inputValue();
    expect(workspace.length > 0, "dossier de travail vide");
    // Fournisseur de la mémoire (14 octobre) : affiché, enregistré, restauré.
    const memRow = p.locator(".field-row", { hasText: "Fournisseur de la mémoire" });
    const beforeMem = await p.evaluate(async () => (await window.__TAURI_INTERNALS__.invoke("get_settings")).llm.memory_provider ?? "");
    expect((await memRow.locator("select").inputValue()) === beforeMem, `fournisseur mémoire affiché, attendu « ${beforeMem} »`);
    const memOptions = await memRow.locator("select option").evaluateAll((els) => els.map((e) => e.value).filter((v) => v));
    const memTarget = memOptions.includes("openrouter") ? "openrouter" : memOptions.find((v) => v !== beforeMem);
    try {
      if (memTarget) {
        await memRow.locator("select").selectOption(memTarget);
        await p.locator(".view-header button", { hasText: "Enregistrer" }).click();
        await p.waitForFunction(
          async (v) => (await window.__TAURI_INTERNALS__.invoke("get_settings")).llm.memory_provider === v,
          memTarget,
          { timeout: 10000 },
        );
      }
    } finally {
      await memRow.locator("select").selectOption(beforeMem);
      await p.locator(".view-header button", { hasText: "Enregistrer" }).click();
      await p.waitForFunction(
        async (v) => (await window.__TAURI_INTERNALS__.invoke("get_settings")).llm.memory_provider === v,
        beforeMem,
        { timeout: 10000 },
      );
    }
    // Attendre le bon message : un toast précédent (skin) peut encore être affiché.
    await p.waitForFunction(
      () => [...document.querySelectorAll(".toast")].some((t) => t.textContent.includes("Paramètres enregistrés")),
      null,
      { timeout: 10000 },
    );
    return "toast affiché";
  });

  await step("Voix : conversation continue et pause de fin de phrase", async () => {
    await nav("Voix");
    await p.waitForTimeout(800);
    const follow = await p.locator(".field-row", { hasText: "Conversation continue" }).locator("select").inputValue();
    expect(follow === "8", `conversation continue : ${follow} s (attendu 8)`);
    const pause = await p.locator(".field-row", { hasText: "Tolérance aux pauses" }).locator("select").inputValue();
    expect(Number(pause) === 1200, `tolérance aux pauses : ${pause} ms (attendu 1200)`);
    const lang = await p.locator(".field-row", { hasText: "Langue" }).locator("select").inputValue();
    expect(lang === "fr", `langue : ${lang}`);
    return `${follow} s, ${pause} ms, langue ${lang}`;
  });

  await step("Voix : synthèse vocale (clé OpenRouter, modèle TTS enregistré puis restauré)", async () => {
    // La clé n'est jamais saisie ici (ce serait la vraie) : on vérifie son
    // état affiché et l'enregistrement du modèle, remis ensuite.
    const before = await p.evaluate(async () => (await window.__TAURI_INTERNALS__.invoke("status")).tts);
    await nav("Voix");
    await p.waitForSelector(".tts-model", { timeout: 10000 });
    const engineCount = await p.locator(".tts-engine-tile").count();
    expect(engineCount === 4, `${engineCount} moteur(s) de voix (attendu 4)`);
    const shown = await p.locator(".tts-model").inputValue();
    expect(shown === before.model, `modèle affiché « ${shown} », attendu « ${before.model} »`);
    const keyState = await p.locator(".tts-key-state").textContent();
    expect(before.has_key === !keyState.includes("Aucune clé"), `état de clé incohérent : has_key=${before.has_key}, « ${keyState} »`);
    // Longueur lue (lecture seule : le réglage de l'utilisateur n'est pas touché).
    await p.waitForFunction(
      () => document.querySelector(".tts-length")?.value !== "",
      null,
      { timeout: 10000 },
    );
    const length = await p.locator(".tts-length").inputValue();
    expect(Number(length) === 900, `longueur lue : ${length} caractères (attendu 900)`);
    const save = () => p.locator(".card", { hasText: "Synthèse vocale" }).locator("button.primary", { hasText: "Enregistrer" }).click();
    try {
      await p.locator(".tts-model").fill("fish-audio/test-ui");
      await save();
      await p.waitForFunction(
        async () => (await window.__TAURI_INTERNALS__.invoke("status")).tts.model === "fish-audio/test-ui",
        null,
        { timeout: 10000 },
      );
    } finally {
      await p.locator(".tts-model").fill(before.model);
      await save();
      await p.waitForFunction(
        async (m) => (await window.__TAURI_INTERNALS__.invoke("status")).tts.model === m,
        before.model,
        { timeout: 10000 },
      );
    }
    return `modèle ${before.model}, clé ${before.has_key ? "présente" : "absente"}`;
  });

  await step("Paramètres : bibliothèque de modèles (liste, test, refus)", async () => {
    // Le modèle vocal se choisit désormais dans l'onglet Voix (parcours
    // dédié plus bas) : ici, seuls la liste, le test et le refus.
    const saved = await p.evaluate(async () => (await window.__TAURI_INTERNALS__.invoke("status")).llm);
    await nav("Réglages");
    await p.locator(".subtab", { hasText: "LLM" }).click();
    await p.waitForSelector(".model-row", { timeout: 40000 });
    expect((await p.locator(".brain-hero").count()) === 1, "carte « Jimy pense avec » absente");
    expect((await p.locator(".brain-single-card").count()) === 1, "carte « Un seul cerveau pour tout » absente");
    const total = await p.locator(".model-row").count();
    // La borne dépend du fournisseur en place : OpenCode Go (~37) et
    // Anthropic en abonnement (~14) listent large, DeepSeek n'expose que
    // « chat » et « reasoner » (2).
    const providerNow = saved.provider || "opencode";
    const minModeles = providerNow === "anthropic" ? 10 : providerNow === "deepseek" ? 2 : 20;
    expect(total >= minModeles, `${total} modèles listés (attendu ≥ ${minModeles})`);
    expect((await p.locator(".model-row.is-main").count()) === 1, "le modèle principal n'est pas repéré dans la liste");
    const ids = await p.$$eval(".model-row code", (els) => els.map((e) => e.textContent ?? ""));
    expect(!ids.some((id) => id.startsWith("opencode-go/")), `identifiants avec fournisseur : ${ids.filter((i) => i.startsWith("opencode-go/")).slice(0, 2).join(", ")}`);
    const main = (await p.locator(".model-current code").first().textContent()) ?? "";

    // Test réel du modèle principal : il doit fonctionner, outils compris.
    const row = p.locator(".model-row.is-main");
    await row.locator("button", { hasText: "Tester" }).click();
    await row.locator(".model-test.good, .model-test.warn, .model-test.bad").waitFor({ timeout: 90000 });
    expect((await row.locator(".model-test.good").count()) === 1, `le modèle principal « ${main} » devrait fonctionner`);

      // Recherche.
      const search = p.locator(".model-toolbar input[type=search]");
      await search.fill("glm");
      const filtered = await p.locator(".model-row").count();
      expect(filtered < total, `recherche « glm » : ${filtered} sur ${total}`);
      await search.fill("");

      // Muse Spark 1.3 n'accepte que le format Responses : muet avant le
      // 6 octobre (Jimmy ne parlait que Chat), il doit fonctionner, outils compris.
      const muse = p.locator(".model-row", { hasText: "muse-spark-1.3-contributor" });
      if ((await muse.count()) > 0) {
        await muse.locator("button", { hasText: "Tester" }).click();
        await muse.locator(".model-test.good, .model-test.warn, .model-test.bad").waitFor({ timeout: 90000 });
        const verdict = (await muse.locator(".model-test").first().textContent()) ?? "";
        expect((await muse.locator(".model-test.good").count()) === 1, `muse-spark-1.3-contributor : « ${verdict} »`);
      }

      // Un modèle qui a échoué au test ne peut pas devenir le modèle principal.
      // Plus aucun modèle du compte n'échoue depuis la prise en charge des trois
      // formats : l'échec est simulé dans les résultats gardés, puis restauré.
      const KEY = "jimmy.llm-tests.v3";
      const savedTests = await p.evaluate((key) => localStorage.getItem(key), KEY);
      try {
        await p.evaluate((key) => {
          const tests = JSON.parse(localStorage.getItem(key) ?? "{}");
          tests["opencode:grok-4.6"] = { model: "grok-4.6", ok: false, tools: false, latency_ms: 0, tools_latency_ms: 0, reply: "", error: "HTTP 400 — échec simulé", tested_at: new Date().toISOString(), at: Date.now() };
          localStorage.setItem(key, JSON.stringify(tests));
        }, KEY);
        await nav("Archives");
        await nav("Réglages");
        await p.locator(".subtab", { hasText: "LLM" }).click();
        await p.waitForSelector(".model-row", { timeout: 40000 });
        const grok = p.locator(".model-row", { hasText: "grok-4.6" });
        if ((await grok.count()) > 0) {
          expect((await grok.getAttribute("class"))?.includes("is-broken"), "grok-4.6 en échec sans être marqué cassé");
          expect(await grok.locator("button", { hasText: "Principal" }).isDisabled(), "un modèle en échec reste choisissable");
        }
      } finally {
        await p.evaluate(([key, value]) => (value === null ? localStorage.removeItem(key) : localStorage.setItem(key, value)), [KEY, savedTests]);
      }
      const after = (await p.locator(".model-current code").first().textContent()) ?? "";
      expect(after === main, `le modèle principal a changé : ${after}`);
    return `${total} modèles, principal « ${main} » testé`;
  });

  await step("Voix : modèle vocal (bibliothèque, choix, retrait)", async () => {
    // Le modèle vocal de l'utilisateur et SON fournisseur, restaurés à la fin
    // quoi qu'il arrive : avant, la suite le remettait à vide (piège 61) et
    // oublier le fournisseur aurait rebranché le vocal sur le principal.
    const saved = await p.evaluate(async () => (await window.__TAURI_INTERNALS__.invoke("status")).llm);
    const savedVoice = saved.voice_model ?? "";
    const savedVoiceProvider = saved.voice_provider || saved.provider || "opencode";
    await nav("Voix");
    await p.waitForSelector(".models-panel .model-row", { timeout: 40000 });
    const total = await p.locator(".models-panel .model-row").count();
    expect(total >= 1, "aucun modèle listé dans l'onglet Voix");
    let choixTeste = false;
    try {
      // Choix d'un modèle vocal fonctionnel, puis retrait.
      const flash = p.locator(".models-panel .model-row", { hasText: "glm-5.3-flash" });
      if ((await flash.count()) > 0) {
        // Déjà le modèle vocal de l'utilisateur : son bouton « Vocal » est
        // grisé et le clic attendait 30 s. On le retire d'abord (le `finally`
        // le remet).
        if ((await flash.getAttribute("class"))?.includes("is-voice")) {
          await p.locator(".models-panel .model-current button", { hasText: "Retirer" }).click();
          await p.waitForSelector(".models-panel .model-row.is-voice", { state: "detached", timeout: 15000 });
        }
        await flash.locator("button", { hasText: "Vocal" }).click();
        // Attendre que CE modèle devienne vocal : un modèle vocal déjà choisi
        // satisfaisait « .model-row.is-voice » tout de suite (échec intermittent).
        await p.waitForFunction(
          () => [...document.querySelectorAll(".models-panel .model-row.is-voice")].some((r) => r.textContent.includes("glm-5.3-flash")),
          null,
          { timeout: 90000 },
        );
        const current = (await p.locator(".models-panel .model-current").textContent()) ?? "";
        expect(current.includes("glm-5.3-flash"), `modèle vocal non affiché : ${current}`);
        // La valeur réellement enregistrée doit être l'identifiant court, jamais « fournisseur/modèle ».
        const chosen = await p.evaluate(async () => (await window.__TAURI_INTERNALS__.invoke("status")).llm.voice_model);
        expect(chosen === "glm-5.3-flash", `modèle vocal enregistré : « ${chosen} »`);
        // Le champ identifiant de la carte suit le choix de la bibliothèque.
        const field = await p.locator(".field-row", { hasText: "Modèle vocal" }).locator("input").inputValue();
        expect(field === "glm-5.3-flash", `champ modèle vocal : « ${field} »`);
        await p.locator(".models-panel .model-current button", { hasText: "Retirer" }).click();
        await p.waitForSelector(".models-panel .model-row.is-voice", { state: "detached", timeout: 15000 });
        choixTeste = true;
      }
    } finally {
      // La suite ne doit jamais laisser un modèle vocal modifié dans ta
      // configuration : on remet exactement le modèle ET le fournisseur relevés
      // au début.
      await p.evaluate(([model, provider]) => window.__TAURI_INTERNALS__.invoke("set_llm_model", { role: "voice", model, provider: model ? provider : null }), [savedVoice, savedVoiceProvider]);
      const restored = await p.evaluate(async () => (await window.__TAURI_INTERNALS__.invoke("status")).llm);
      if ((restored.voice_model ?? "") !== savedVoice) throw new Error(`modèle vocal non restauré : « ${restored.voice_model} » au lieu de « ${savedVoice} »`);
      if (savedVoice && (restored.voice_provider || restored.provider) !== savedVoiceProvider) {
        throw new Error(`fournisseur vocal non restauré : « ${restored.voice_provider} » au lieu de « ${savedVoiceProvider} »`);
      }
    }
    return `${total} modèles listés${choixTeste ? ", choix vocal vérifié" : " (fournisseur sans glm-5.3-flash : choix non testé)"}`;
  });

  await step("Paramètres : fournisseurs (liste, clé masquée, ajout et retrait d'un personnalisé)", async () => {
    await nav("Réglages");
    await p.locator(".subtab", { hasText: "LLM" }).click();
    await p.waitForSelector(".provider-row", { timeout: 20000 });
    const rows = p.locator(".card", { hasText: "Fournisseurs" }).locator(".provider-row:not(.provider-add)");
    const before = await rows.count();
    expect(before >= 5, `${before} fournisseurs listés (attendu ≥ 5)`);
    // Les cinq intégrés existent, Claude est en format Messages. Attention :
    // « Anthropic » ne doit pas se chercher en texte libre — le libellé d'un
    // option des sélecteurs le contient, et tous les rows « ont ce texte ».
    const claude = rows.filter({ has: p.locator("code", { hasText: "anthropic" }) });
    expect((await claude.count()) === 1, "Anthropic absent de la liste");
    expect((await claude.locator("select.provider-protocol").inputValue()) === "messages", "Anthropic pas en format messages");
    // La méthode d'accès existe : clé API ou abonnement Claude (session Claude Code).
    const auth = claude.locator("select.provider-auth");
    expect((await auth.count()) === 1, "sélecteur de méthode d'accès absent chez Anthropic");
    const authOptions = await auth.locator("option").allTextContents();
    expect(authOptions.some((o) => o.includes("Abonnement Claude")), `méthode d'abonnement absente : ${authOptions.join(", ")}`);
    // Le fournisseur du modèle principal porte le badge « principal ».
    expect((await rows.filter({ has: p.locator(".badge.main", { hasText: "principal" }) }).count()) >= 1, "aucun fournisseur marqué principal");
    // La clé ne remonte jamais : aucun champ mot de passe ne contient de valeur.
    const filled = await p.$$eval(".provider-row .provider-key", (els) => els.filter((e) => e.value.length > 0).length);
    expect(filled === 0, `${filled} champs clé contiennent une valeur (jamais renvoyée par Rust)`);

    // Ajout d'un fournisseur personnalisé (sans test réseau), puis retrait.
    const add = p.locator(".provider-add");
    await add.locator("input").nth(0).fill("Suite UI Locale");
    await add.locator("input").nth(1).fill("http://127.0.0.1:1234/v1");
    await add.locator("button", { hasText: "Ajouter" }).click();
    await p.waitForFunction(
      () => [...document.querySelectorAll(".provider-row:not(.provider-add)")].some((r) => r.textContent.includes("Suite UI Locale")),
      null,
      { timeout: 20000 },
    );
    const after = await rows.count();
    expect(after === before + 1, `après ajout : ${after} (attendu ${before + 1})`);
    const custom = rows.filter({ hasText: "Suite UI Locale" });
    await custom.locator("button", { hasText: "Supprimer" }).click();
    await p.waitForSelector(".provider-row", { state: "attached", timeout: 20000 });
    await p.waitForFunction(
      (n) => document.querySelectorAll(".provider-row:not(.provider-add)").length === n,
      before,
      { timeout: 20000 },
    );
    return `${before} fournisseurs, ajout/retrait OK`;
  });

  await step("Voix : bibliothèque de voix (liste, recherche, choix)", async () => {
    const invoke = (cmd, args) => p.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args]);
    const before = await invoke("tts_voices", {});
    const saved = [...before.presets, ...before.library].find((v) => v.id === before.current);
    try {
      // Dans l'onglet Voix, là où l'utilisateur la cherche.
      await nav("Voix");
      await p.waitForSelector(".voices-panel .voice-row", { timeout: 10000 });
      const rows = await p.locator(".voices-panel .voice-list").first().locator(".voice-row").count();
      expect(rows >= 3, `${rows} voix dans la bibliothèque (attendu ≥ 3)`);
      // Recherche dans le catalogue public (réseau).
      await p.locator(".voice-search input").fill("narrateur");
      await p.locator(".voice-search button").click();
      await p.waitForFunction(() => document.querySelectorAll(".voice-list")[1]?.querySelectorAll(".voice-row").length > 0, null, {
        timeout: 15000,
      });
      // Choisir Féminine : la valeur réellement enregistrée doit suivre.
      const feminine = p.locator(".voice-list").first().locator(".voice-row", { hasText: "Féminine" });
      await feminine.locator("button", { hasText: "Choisir" }).click();
      await p.waitForFunction(() => document.querySelector(".voice-row.is-main")?.textContent.includes("Féminine"), null, { timeout: 5000 });
      const after = await invoke("tts_voices", {});
      expect(after.current === "5567200c7d8341738f0892bbacd3be3c", `voix enregistrée : ${after.current}`);
      return `${rows} voix, recherche OK, choix enregistré`;
    } finally {
      // La voix de l'utilisateur est remise, quoi qu'il arrive (piège 61).
      if (saved) await invoke("tts_set_voice", { voice: saved });
      const restored = await invoke("tts_voices", {});
      if (restored.current !== before.current) throw new Error(`voix non restaurée : ${restored.current}`);
    }
  });

  await step("Historique : ouvrir une session", async () => {
    await nav("Archives");
    await p.waitForSelector(".list-row", { timeout: 5000 });
    await p.locator(".list-row").first().locator("button", { hasText: "Ouvrir" }).click();
    await p.waitForTimeout(1000);
    const title = await p.locator(".topbar h1").textContent();
    const bubbles = await p.locator(".stream .bubble").count();
    expect(title === "Discuter" && bubbles > 0, `${title}, ${bubbles} bulles`);
    return `${bubbles} bulles`;
  });

  // Historique groupé par projet + recherche instantanée + Ctrl+K.
  await step("Historique : projets groupés, recherche, Ctrl+K", async () => {
    const repoName = "jimmy-agent-personnel";
    await nav("Archives");
    // Groupes : un par projet, le dépôt de travail du test est groupé.
    const groups = await p.$$eval(".history-group", (els) => els.map((e) => e.textContent));
    expect(groups.includes(repoName), `groupes : ${groups.join(" | ")}`);
    // La recherche filtre en direct : une lettre absente de tous les titres
    // du dépôt ramène son groupe à zéro session visible.
    await p.locator(".history-search").fill("zzz-champ-absent");
    await p.waitForSelector(".empty", { timeout: 5000 });
    const leaves = await p.locator(".list-row").count();
    expect(leaves === 0, `${leaves} session(s) restent avec un filtre vide`);
    // Ctrl+K (depuis la vue Mémoire) ramène sur la recherche de sessions.
    await nav("Mémoire");
    await p.keyboard.press("Control+k");
    await p.waitForSelector(".history-search", { timeout: 5000 });
    await p.locator(".history-search").fill("");
    const full = await p.locator(".list-row").count();
    expect(full > 0, "le filtre remis à zéro : aucune session listée");
    return `${groups.length} groupe(s), recherche et Ctrl+K OK`;
  });

  // Lot D1 (21 octobre) : la réponse d'une conversation quittée en cours de
  // route ne s'affiche jamais dans la nouvelle ; elle reste dans la sienne.
  await step("Sessions : une réponse reste dans sa conversation", async () => {
    const invoke = (cmd, args) => p.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args ?? {}]);
    await nav("Discuter");
    await p.locator(".chat-header button", { hasText: "Nouvelle session" }).click();
    await p.locator(".composer-input").fill("Réponds uniquement par le mot : banane");
    await p.keyboard.press("Enter");
    await p.waitForSelector(".bubble.pending", { timeout: 5000 });
    // Conversation quittée avant la réponse.
    await p.locator(".chat-header button", { hasText: "Nouvelle session" }).click();
    let origin = null;
    for (let i = 0; i < 60 && !origin; i++) {
      await p.waitForTimeout(1000);
      const s = newest(await invoke("sessions"));
      const msgs = s ? await invoke("session_messages", { sessionId: s.id }) : [];
      if (msgs.some((m) => m.role === "assistant" && /banane/i.test(m.content))) origin = s;
    }
    expect(origin, "pas de réponse « banane » en 60 s");
    await p.waitForTimeout(800);
    const leaked = await p.locator(".stream .bubble.assistant", { hasText: /banane/i }).count();
    expect(leaked === 0, `la réponse s'affiche dans la nouvelle conversation (${leaked} bulle(s))`);
    // Étape 1 de la refonte : la notification propose « Ouvrir », la liste
    // montre la réponse non lue, un clic ouvre la conversation et la marque lue.
    await p.waitForSelector(".toast .toast-action", { timeout: 8000 });
    // Fenêtre étroite : la liste est repliée d'office ; on la déplie.
    if (await p.locator(".conv-rail.collapsed").count()) await p.locator(".conv-rail .conv-toggle").first().click();
    const row = p.locator(`.conv-row`, { hasText: /banane/i }).first();
    await row.locator(".conv-unread").waitFor({ timeout: 8000 });
    await row.click();
    await p.waitForSelector(".stream .bubble.assistant", { timeout: 8000 });
    const shown = await p.locator(".stream .bubble.assistant", { hasText: /banane/i }).count();
    expect(shown > 0, "la conversation ouverte depuis la liste ne montre pas la réponse");
    await p.waitForFunction(() => {
      const r = [...document.querySelectorAll(".conv-row")].find((e) => /banane/i.test(e.textContent ?? ""));
      return r && !r.querySelector(".conv-unread") && r.classList.contains("active");
    }, null, { timeout: 8000 });
    return "réponse gardée dans sa conversation ; « Ouvrir », non-lu, ouverture depuis la liste et lecture OK";
  });

  // Lot A (21 octobre) : recherche dans le contenu, épingler, renommer, archiver.
  await step("Historique : épingler, renommer, archiver, chercher dans le contenu", async () => {
    const invoke = (cmd, args) => p.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args ?? {}]);
    // La session « banane » du parcours précédent, cherchée par son titre :
    // une autre tâche a pu toucher une session plus récente entre-temps.
    const origin = newest((await invoke("sessions")).filter((s) => /banane/i.test(s.title)));
    expect(origin, "session de test « banane » introuvable");
    const row = () => p.locator(`.list-row[data-session="${origin.id}"]`);
    await nav("Archives");
    try {
      await row().waitFor({ timeout: 5000 });
      // Épingler : la session passe dans le groupe « Épinglées ».
      await row().locator("button", { hasText: "Épingler" }).click();
      await p.waitForSelector(`.history-pinned ~ .list-row[data-session="${origin.id}"].pinned`, { timeout: 5000 });
      // Renommer : titre verrouillé, plus réécrit automatiquement.
      p.once("dialog", (d) => d.accept("Test UI renommée"));
      await row().locator("button", { hasText: "Renommer" }).click();
      await p.waitForFunction((id) => document.querySelector(`.list-row[data-session="${id}"] strong`)?.textContent === "Test UI renommée", origin.id, { timeout: 5000 });
      // Recherche dans le contenu : la phrase n'est plus dans le titre.
      await p.locator(".history-search").fill("uniquement par le mot");
      await row().waitFor({ timeout: 5000 });
      await p.locator(".history-search").fill("");
      await p.waitForTimeout(500);
      // Archiver : sort des actives, présente dans les archivées, puis restaurée.
      await row().locator("button", { hasText: "Archiver" }).click();
      await row().waitFor({ state: "detached", timeout: 5000 });
      await p.locator(".history-archived-toggle").click();
      await row().waitFor({ timeout: 5000 });
      await row().locator("button", { hasText: "Désarchiver" }).click();
      await row().waitFor({ state: "detached", timeout: 5000 });
      await p.locator(".history-archived-toggle").click();
      const info = await invoke("session_get", { sessionId: origin.id });
      expect(info.pinned && !info.archived && info.title === "Test UI renommée", JSON.stringify(info));
    } finally {
      // Session de test supprimée quoi qu'il arrive.
      await invoke("delete_session", { sessionId: origin.id });
    }
    return "épinglée, renommée, trouvée par son contenu, archivée puis restaurée";
  });

  await step("Mémoire : libellés traduits", async () => {
    await nav("Mémoire");
    await p.waitForTimeout(800);
    const tags = await p.$$eval(".tag", (els) => els.map((e) => e.textContent));
    expect(!tags.some((t) => /semantic|procedural|episodic/.test(t)), `tags : ${tags.join(", ")}`);
    return tags.join(", ") || "aucun souvenir";
  });

  await step("Skills : sous-onglet des serveurs MCP", async () => {
    await nav("Outils");
    await p.waitForSelector(".subtab.active", { timeout: 5000 });
    await p.locator(".subtab", { hasText: "Serveurs MCP" }).click();
    // La liste affichée doit correspondre à ce que l'application rapporte.
    const servers = await p.evaluate(() => window.__TAURI_INTERNALS__.invoke("mcp_servers"));
    if (servers.length === 0) {
      await p.waitForSelector(".empty", { timeout: 5000 });
      return "aucun serveur configuré";
    }
    await p.waitForSelector(".mcp-server", { timeout: 5000 });
    const rows = await p.locator(".mcp-server").count();
    expect(rows === servers.length, `${rows} ligne(s) affichée(s) pour ${servers.length} serveur(s)`);
    // Aucune clé en clair : après un nom de secret, seule la valeur masquée.
    const commands = await p.$$eval(".mcp-server pre", (els) => els.map((e) => e.textContent ?? ""));
    const leak = commands.find((c) => /(key|token|secret|password|authorization)\s*[:=]\s*(?!••••)\S/i.test(c));
    expect(!leak, "secret affiché en clair dans une commande MCP");
    // Le sous-onglet Skills revient.
    await p.locator(".subtab", { hasText: "Skills" }).click();
    expect(await p.locator(".mcp-server").first().isHidden(), "la liste MCP reste visible sur l'onglet Skills");
    return servers.map((s) => `${s.name} (${s.state}, ${s.tools.length} outils)`).join(", ");
  });

  // Secrets (12 octobre) : une clé du coffre `.secrets` que Jimy croise par
  // accident (fichier anodin, liste de processus…) arrive masquée au modèle et
  // dans l'historique. Canari déposé dans le coffre par `test-ui.ps1` : une
  // lecture volontaire d'un fichier de clé, le modèle la masque déjà lui-même.
  await step("Sécurité : une clé du coffre lue par Jimy arrive masquée", async () => {
    const invoke = (cmd, args) => p.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args ?? {}]);
    const cle = process.env.JIMMY_UI_CANARI;
    expect(Boolean(cle), "canari absent (lancer par test-ui.ps1)");
    const fichier = path.join(OUT, "notes-reunion.txt");
    require("fs").writeFileSync(fichier, `Compte rendu\nréférence du dossier : ${cle}\nfin\n`);
    await nav("Discuter");
    await p.locator(".chat-header button", { hasText: "Nouvelle session" }).click();
    await p.locator(".composer-input").fill(`Lis le fichier ${fichier} avec read_file, puis réponds uniquement : lu.`);
    await p.keyboard.press("Enter");
    await p.waitForSelector(".bubble.pending", { timeout: 5000 });
    await attendreReponse(120_000);
    const session = newest(await invoke("sessions"));
    // `session_messages` n'expose pas les résultats d'outils : lecture seule
    // de la base (la clé passe par l'environnement, jamais en argument).
    const { execFileSync } = require("child_process");
    const db = path.resolve(__dirname, "..", "..", "data", "jimmy.db");
    const script = [
      "import os, sqlite3, sys",
      "con = sqlite3.connect('file:' + sys.argv[1] + '?mode=ro', uri=True)",
      "rows = [c or '' for (c,) in con.execute(\"SELECT content FROM messages WHERE session_id = ? AND role = 'tool'\", (sys.argv[2],))]",
      "print(len(rows), sum(os.environ['CLE'] in c for c in rows), sum('secret masqué' in c for c in rows))",
    ].join("\n");
    const sortie = execFileSync("python", ["-I", "-c", script, db, session.id], { env: { ...process.env, CLE: cle, PYTHONIOENCODING: "utf-8" } })
      .toString()
      .trim();
    const [outils, fuites, masques] = sortie.split(/\s+/).map(Number);
    if (session?.title?.startsWith("Lis le fichier")) await invoke("delete_session", { sessionId: session.id });
    require("fs").rmSync(fichier, { force: true });
    expect(outils > 0, "aucun outil appelé");
    expect(fuites === 0, "la clé est en clair dans l'historique");
    expect(masques > 0, "pas de marque de masquage");
    return `${outils} résultat(s) d'outil, clé masquée`;
  });

  await step("Diagnostic : toutes les vérifications passent", async () => {
    await nav("Diagnostic");
    await p.waitForSelector(".ok-note, .warn-note", { timeout: 15000 });
    return await p.locator(".ok-note, .warn-note").first().textContent();
  });

  // Captures de fin : une étape comme les autres. Si la page a disparu (veille
  // du PC, 22 octobre), l'échec est noté mais la suite va jusqu'au bout :
  // résultats affichés et réglages de l'utilisateur restaurés.
  await step("Captures de fin (Voix, Discuter)", async () => {
    await nav("Voix");
    await p.screenshot({ path: path.join(OUT, "suite-voix.png") });
    await nav("Discuter");
    await p.screenshot({ path: path.join(OUT, "suite-chat.png") });
    return "2 captures";
  });

  await step("Mise à jour : pastille quand une mise à jour attend", async () => {
    const fs = require("fs");
    const stateFile = path.resolve(__dirname, "..", "..", "data", "update_state.json");
    const already = await p.evaluate(() =>
      window.__TAURI_INTERNALS__.invoke("status").then((s) => s.update.pending),
    );
    if (!already) {
      // Simule une mise à jour en attente : la barre latérale se rafraîchit
      // toutes les 15 s via `status`. La boucle de fond peut réécrire le
      // fichier pile entre l'apparition et le clic : écrire → attendre →
      // cliquer est rejoué jusqu'à 3 fois.
      const fake = JSON.stringify({
        last_check: new Date().toISOString(),
        local: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        remote: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        pending: true,
        notified: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
      });
      const backup = fs.existsSync(stateFile) ? fs.readFileSync(stateFile, "utf-8") : null;
      try {
        let clicked = false;
        for (let attempt = 0; attempt < 3 && !clicked; attempt++) {
          fs.writeFileSync(stateFile, fake);
          const shown = await p.waitForSelector(".update-badge", { timeout: 25000 }).then(() => true).catch(() => false);
          if (!shown) continue;
          await p.screenshot({ path: path.join(OUT, "suite-badge.png") });
          clicked = await p.locator(".update-badge").click({ timeout: 8000 }).then(() => true).catch(() => false);
        }
        expect(clicked, "pastille jamais cliquable en 3 essais");
      } finally {
        // Restaure l'état réel (arbre à jour : plus d'alerte).
        if (backup === null) fs.rmSync(stateFile, { force: true });
        else fs.writeFileSync(stateFile, backup);
      }
      await p.waitForFunction(() => !document.querySelector(".update-badge"), null, { timeout: 25000 });
    } else {
      await p.waitForSelector(".update-badge", { timeout: 10000 });
      await p.locator(".update-badge").click({ timeout: 8000 });
    }
    await p.waitForFunction(() => document.querySelector(".topbar h1")?.textContent === "Archives", null, { timeout: 5000 });
    await nav("Discuter");
    return already ? "vraie alerte : pastille et clic vérifiés" : "pastille simulée, clic vers Historique, disparue après restauration";
  });

  // Version anglaise (22 octobre) : l'interface passe en anglais, les onglets
  // et le Chat suivent ; la langue de l'utilisateur est rendue à la fin.
  await step("Langue : interface en anglais puis restaurée", async () => {
    const invoke = (cmd, args) => p.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args ?? {}]);
    const setLang = async (value) => {
      const s = await invoke("get_settings");
      s.ui.language = value;
      await invoke("save_settings", { settings: s });
      await p.reload();
      await p.waitForSelector(".nav-item", { timeout: 20000 });
      await p.waitForTimeout(800);
    };
    const before = (await invoke("get_settings")).ui.language;
    try {
      await setLang("en");
      const labels = await p.$$eval(".nav-item", (els) => els.map((e) => e.textContent.trim()));
      expect(["Chat", "Voice", "Memory", "Tools", "Appearance", "Archives", "Settings", "Diagnostics"].every((l) => labels.includes(l)), `onglets : ${labels.join(", ")}`);
      await p.locator(".nav-item", { hasText: "Chat" }).click();
      await p.waitForTimeout(500);
      const send = (await p.locator(".send-label").textContent()) ?? "";
      const html = await p.evaluate(() => document.documentElement.lang);
      expect(send === "Send" && html === "en", `bouton « ${send} », lang=${html}`);
      await p.locator(".nav-item", { hasText: "Voice" }).click();
      await p.waitForSelector(".card h3", { timeout: 5000 });
      const voiceH3 = await p.$$eval(".card h3", (els) => els.map((e) => e.textContent?.trim()));
      expect(voiceH3.includes("Voice activation"), `titres Voice : ${voiceH3.join(", ")}`);
      await p.locator(".nav-item", { hasText: "Settings" }).click();
      await p.waitForSelector(".card h3", { timeout: 5000 });
      const settingsH3 = await p.$$eval(".card h3", (els) => els.map((e) => e.textContent?.trim()));
      expect(settingsH3.includes("Workspace"), `titres Settings : ${settingsH3.join(", ")}`);
      const doc = await invoke("doctor");
      const names = doc.checks.map((c) => c.name);
      expect(names.includes("OpenCode Go key") && names.includes("Speech recognition model"), `doctor EN : ${names.join(", ")}`);
    } finally {
      await setLang(before);
    }
    const back = await p.$$eval(".nav-item", (els) => els.map((e) => e.textContent.trim()));
    expect(back.includes("Discuter") || before === "en", `langue non restaurée : ${back.join(", ")}`);
    return `onglets et Chat en anglais, langue « ${before} » restaurée`;
  });

  await step("Aucune erreur JavaScript", async () => {
    expect(errors.length === 0, errors.join(" | "));
  });

  // Écoute remise comme avant la suite (et la préférence de lancement avec).
  await p.evaluate(async (on) => {
    const status = await window.__TAURI_INTERNALS__.invoke("voice_status");
    if (on && !status.running) await window.__TAURI_INTERNALS__.invoke("voice_start");
    if (!on && status.running) await window.__TAURI_INTERNALS__.invoke("voice_stop");
  }, ecouteInitiale).catch((e) => results.push(`ÉCHEC  Écoute non restaurée — ${e.message.split("\n")[0]}`));

  // Interrupteurs « Micro » et « Jimy parle » rendus comme avant la suite.
  await p
    .evaluate(async (avant) => {
      for (const scope of ["listen", "speech"]) {
        await window.__TAURI_INTERNALS__.invoke("voice_set_enabled", { scope, enabled: avant[scope] });
      }
    }, interrupteurs)
    .catch((e) => results.push(`ÉCHEC  Interrupteurs non restaurés — ${e.message.split("\n")[0]}`));

  console.log(results.join("\n"));
  await browser.close();
  process.exit(results.some((r) => r.startsWith("ÉCHEC")) ? 1 : 0);
})();
