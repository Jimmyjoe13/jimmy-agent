// Suite de tests UI de bout en bout, sur la vraie application (CDP).
// Chaque parcours affiche OK / ÉCHEC avec le détail ; code de sortie 1 si un
// parcours échoue.
const { chromium } = require("playwright-core");
const path = require("path");
const OUT = path.resolve(__dirname, "..", "..", "data", "ui-test");
require("fs").mkdirSync(OUT, { recursive: true });

const results = [];
async function step(name, fn) {
  try {
    const detail = await fn();
    results.push(`OK     ${name}${detail ? ` — ${detail}` : ""}`);
  } catch (e) {
    results.push(`ÉCHEC  ${name} — ${e.message.split("\n")[0]}`);
  }
}

(async () => {
  const browser = await chromium.connectOverCDP("http://127.0.0.1:9222");
  const p = browser.contexts()[0].pages().find((pg) => pg.url().includes("tauri.localhost"));
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

  await step("Navigation : une seule vue montée par page", async () => {
    for (const label of ["Chat", "Voix", "Historique", "Mémoire", "Skills", "Skin", "Paramètres", "Diagnostic"]) {
      await nav(label);
      const n = await p.$$eval("main.content > *", (els) => els.length);
      expect(n === 1, `${label} : ${n} vues`);
      const title = await p.locator(".topbar h1").textContent();
      expect(title === label, `titre « ${title} » au lieu de « ${label} »`);
    }
    return "8 vues";
  });

  await step("Chat : envoi, attente visible, réponse", async () => {
    await nav("Chat");
    await p.locator("button", { hasText: "Nouvelle session" }).click();
    await p.locator(".composer-input").fill("Réponds uniquement par le mot : banane");
    await p.keyboard.press("Enter");
    await p.waitForSelector(".bubble.pending", { timeout: 5000 });
    const busy = await p.locator(".composer button.primary").textContent();
    expect(busy.includes("travaille"), `bouton pendant l'attente : « ${busy} »`);
    await p.waitForSelector(".bubble.pending", { state: "detached", timeout: 120000 });
    const last = await p.locator(".stream .bubble").last();
    const cls = await last.getAttribute("class");
    const text = (await last.textContent()).trim();
    expect(cls.includes("assistant"), `dernière bulle : ${cls} « ${text} »`);
    const free = await p.locator(".composer button.primary").textContent();
    expect(free === "Envoyer", `bouton après réponse : « ${free} »`);
    const labels = await p.$$eval(".stream .bubble", (els) => els.map((e) => e.textContent));
    expect(!labels.some((t) => /^(Jimmy|Vous)/.test(t)), "libellé en double dans une bulle");
    return `« ${text.slice(0, 60)} »`;
  });

  await step("Chat : la pastille revient à « prêt » après la réponse", async () => {
    await p.waitForFunction(() => document.querySelector(".topbar .chip")?.textContent === "prêt", null, { timeout: 15000 });
    return "prêt";
  });

  await step("Chat : la session est reprise après navigation", async () => {
    const before = await p.locator(".stream .bubble").count();
    await nav("Mémoire");
    await nav("Chat");
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
    const latest = async () => (await p.evaluate(() => window.__TAURI_INTERNALS__.invoke("sessions")))[0]?.id;
    const sessionBefore = await latest();
    await p.locator(".composer-input").fill("Quel mot viens-tu d'écrire juste avant ? Réponds en un seul mot.");
    await p.keyboard.press("Enter");
    await p.waitForSelector(".bubble.pending", { timeout: 5000 });
    await p.waitForSelector(".bubble.pending", { state: "detached", timeout: 120000 });
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
    const session = (await invoke("sessions"))[0];
    // Le sélecteur de dossier natif ne se pilote pas : on rattache le projet
    // par la commande qu'il appelle, puis on vérifie tout le reste à l'écran.
    await invoke("session_set_project", { sessionId: session.id, project: repo });
    await nav("Mémoire");
    await nav("Chat");
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
    await p.waitForSelector(".bubble.pending", { state: "detached", timeout: 120000 });
    const messages = await invoke("session_messages", { sessionId: session.id });
    const answer = messages.filter((m) => m.role === "assistant").pop()?.content ?? "";
    expect(answer.toLowerCase().includes(repoName.toLowerCase()), `dossier de travail annoncé : « ${answer} »`);
    return `projet ${repoName}, ${names.length} entrées, aperçu et dossier de travail OK`;
  });

  // Mention « @ » (façon Codex) : @ + lettres → menu des fichiers du projet,
  // Entrée insère le chemin relatif SANS envoyer le message.
  await step("Chat : « @ » cite un fichier du projet", async () => {
    const input = p.locator(".composer-input");
    await input.fill("@HAND");
    await p.waitForSelector(".mention-menu:not([hidden]) .mention-item", { timeout: 5000 });
    const items = await p.$$eval(".mention-menu .mention-name", (els) => els.map((e) => e.textContent));
    expect(items.some((n) => n === "HANDOFF.md"), `menu « @ » : ${items.join(", ")}`);
    await p.keyboard.press("ArrowDown"); // changement d'item (Index 0 → 1 puis retour si un seul item)
    await p.keyboard.press("Enter");
    const value = await input.inputValue();
    expect(value.startsWith("@HANDOFF.md "), `inséré après Entrée : « ${value} »`);
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
    const target = "C:\\Users\\jimmy\\Projet\\jimmy-agent-personnel\\HANDOFF.md:3";
    const input = p.locator(".composer-input");
    // L'attente après l'envoi : une seule demande, une réponse pilote.
    await input.fill(`Réponds UNIQUEMENT par ce chemin, copié exactement, sans phrase : ${target}`);
    await p.keyboard.press("Enter");
    await p.waitForSelector(".bubble.pending", { timeout: 5000 });
    await p.waitForSelector(".bubble.pending", { state: "detached", timeout: 180000 });
    await p.waitForSelector(".bubble.assistant .msg-path", { timeout: 5000 });
    const label = (await p.locator(".bubble.assistant .msg-path").textContent()) ?? "";
    expect(label.replace(/\D/g, "").includes("3"), `étiquette de la puce : « ${label} »`);
    await p.locator(".bubble.assistant .msg-path").first().click();
    await p.waitForSelector(".file-modal-card", { timeout: 5000 });
    const head = (await p.locator(".file-modal-head strong").textContent()) ?? "";
    expect(head.includes("HANDOFF.md"), `aperçu ouvert dans : « ${head} »`);
    await p.waitForSelector(".file-modal-line.target", { timeout: 5000 });
    await p.keyboard.press("Escape");
    await p.waitForSelector(".file-modal", { state: "detached", timeout: 5000 });
    return `puce « ${label} », aperçu + ligne surlignée`;
  });

  // Bloc « travaux » (façon Codex) : les fichiers écrits par Jimmy dans le
  // tour apparaissent sous la réponse, en puces cliquables. Ici data/tests-ui/
  // (dossier de données local, jamais commité) : le diff silencieux est
  // acceptable, seules les puces sont vérifiées.
  await step("Chat : le bloc « travaux » liste les fichiers écrits", async () => {    const note = (await p.evaluate(() => window.__TAURI_INTERNALS__.invoke("paths_info"))).data;
    const fichier = `${note}\\tests-ui\\reussite-${Date.now() % 10_000}.txt`;
    const input = p.locator(".composer-input");
    await input.fill(`Écris dans « ${fichier} » la ligne unique : test travaux. Ne renvoie que la confirmation en une phrase.`);
    await p.keyboard.press("Enter");
    await p.waitForSelector(".bubble.pending", { timeout: 5000 });
    await p.waitForSelector(".bubble.pending", { state: "detached", timeout: 180000 });
    await p.waitForSelector(".bubble.assistant .work-block .work-file", { timeout: 10000 });
    const chip = (await p.locator(".work-file").textContent()) ?? "";
    expect(chip.endsWith(".txt"), `puce du bloc travaux : « ${chip} »`);
    return `bloc travaux visible : « ${chip} »`;
  });

  // Cas réel du 4 octobre (piège 69) : un projet choisi hors d'une
  // conversation enregistrée se perdait au changement d'onglet.
  await step("Chat : le projet d'une nouvelle conversation survit à la navigation", async () => {
    const repo = (await p.evaluate(() => window.__TAURI_INTERNALS__.invoke("paths_info"))).app;
    const repoName = repo.split(/[\\/]/).filter(Boolean).pop();
    const nameIs = (n) => p.waitForFunction((x) => document.querySelector(".project-name")?.textContent === x, n, { timeout: 5000 });
    // Nouvelle conversation : elle garde le projet en cours.
    await p.locator("button", { hasText: "Nouvelle session" }).click();
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
    await nav("Chat");
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
    await p.waitForSelector(".bubble.pending", { state: "detached", timeout: 15000 });
    const ms = Date.now() - t0;
    const text = ((await p.locator(".stream .bubble.assistant").last().textContent()) ?? "").trim();
    expect(/Arrêté/.test(text), `réponse après l'arrêt : « ${text} »`);
    expect(await p.locator(".stop-button").isHidden(), "le bouton Arrêter reste visible");
    // La conversation de test ne reste pas dans l'historique de l'utilisateur.
    const latest = (await invoke("sessions"))[0];
    if (latest?.title?.startsWith("Lis un par un")) await invoke("delete_session", { sessionId: latest.id });
    return `arrêté en ${ms} ms`;
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
    await nav("Chat");
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
    const chip = (await p.locator(".topbar .chip").textContent()) ?? "";
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

  await step("Skin : changement appliqué et bouton actif mis à jour", async () => {
    await nav("Skin");
    await p.locator("button", { hasText: "Fennec" }).click();
    await p.waitForTimeout(1500);
    const cls = await p.locator("button", { hasText: "Fennec" }).getAttribute("class");
    expect(cls === "primary", `bouton Fennec : ${cls}`);
    await p.locator("button", { hasText: "Renard roux" }).click();
    await p.waitForTimeout(1500);
    const back = await p.locator("button", { hasText: "Renard roux" }).getAttribute("class");
    expect(back === "primary", `retour renard : ${back}`);
    const dodge = p.locator(".card", { hasText: "Comportement" }).locator("input[type=checkbox]");
    expect((await dodge.count()) === 1, "interrupteur d'esquive absent");
    const states = await p.$$eval(".state-chip", (els) => els.map((e) => e.textContent));
    expect(states.includes("j'écoute"), `états non traduits : ${states.join(", ")}`);
    return "fennec puis renard";
  });

  await step("Paramètres : enregistrement confirmé", async () => {
    await nav("Paramètres");
    await p.waitForTimeout(800);
    const lang = await p.locator(".field-row", { hasText: "Langue" }).locator("select").inputValue();
    expect(lang === "fr", `langue : ${lang}`);
    await p.locator(".view-header button", { hasText: "Enregistrer" }).click();
    // Attendre le bon message : un toast précédent (skin) peut encore être affiché.
    await p.waitForFunction(
      () => [...document.querySelectorAll(".toast")].some((t) => t.textContent.includes("Paramètres enregistrés")),
      null,
      { timeout: 10000 },
    );
    return "toast affiché";
  });

  await step("Paramètres : conversation continue et pause de fin de phrase", async () => {
    await nav("Paramètres");
    await p.waitForTimeout(800);
    const follow = await p.locator(".field-row", { hasText: "Conversation continue" }).locator("select").inputValue();
    expect(follow === "8", `conversation continue : ${follow} s (attendu 8)`);
    const pause = await p.locator(".field-row", { hasText: "Pause qui termine" }).locator("input").inputValue();
    expect(Number(pause) === 700, `pause de fin de phrase : ${pause} ms (attendu 700)`);
    return `${follow} s, ${pause} ms`;
  });

  await step("Paramètres : bibliothèque de modèles (liste, test, refus, choix vocal)", async () => {
    // Le modèle vocal de l'utilisateur, restauré à la fin quoi qu'il arrive.
    // Avant, la suite le remettait à vide : elle a effacé MiMo deux fois le
    // 4 octobre (piège 61).
    const savedVoice = await p.evaluate(async () => (await window.__TAURI_INTERNALS__.invoke("status")).llm.voice_model ?? "");
    await nav("Paramètres");
    await p.waitForSelector(".model-row", { timeout: 40000 });
    const total = await p.locator(".model-row").count();
    expect(total >= 20, `${total} modèles listés (attendu ≥ 20)`);
    expect((await p.locator(".model-row.is-main").count()) === 1, "le modèle principal n'est pas repéré dans la liste");
    const ids = await p.$$eval(".model-row code", (els) => els.map((e) => e.textContent ?? ""));
    expect(!ids.some((id) => id.includes("/")), `identifiants avec fournisseur : ${ids.filter((i) => i.includes("/")).slice(0, 2).join(", ")}`);
    const main = (await p.locator(".model-current code").first().textContent()) ?? "";

    try {
      // Test réel du modèle principal : il doit fonctionner, outils compris.
      const row = p.locator(".model-row.is-main");
      await row.locator("button", { hasText: "Tester" }).click();
      await row.locator(".model-test.good, .model-test.warn, .model-test.bad").waitFor({ timeout: 90000 });
      expect((await row.locator(".model-test.good").count()) === 1, `le modèle principal « ${main} » devrait fonctionner`);

      // Recherche.
      const search = p.locator(".model-toolbar input[type=search]");
      await search.fill("glm");
      const filtered = await p.locator(".model-row").count();
      expect(filtered > 0 && filtered < total, `recherche « glm » : ${filtered} sur ${total}`);
      await search.fill("");

      // Un modèle que le fournisseur refuse ne peut pas devenir le modèle principal.
      const grok = p.locator(".model-row", { hasText: "grok-4.6" });
      if ((await grok.count()) > 0) {
        const button = grok.locator("button", { hasText: "Principal" });
        if (await button.isDisabled()) {
          // Déjà marqué « cassé » par un test précédent (résultats gardés dans
          // le localStorage) : le bouton est grisé, le refus est déjà acquis.
          expect((await grok.getAttribute("class"))?.includes("is-broken"), "grok-4.6 grisé sans être marqué cassé");
        } else {
          await button.click();
          await p.waitForFunction(
            () => [...document.querySelectorAll(".toast")].some((t) => t.textContent.includes("ne répond pas")),
            null,
            { timeout: 90000 },
          );
        }
        const after = (await p.locator(".model-current code").first().textContent()) ?? "";
        expect(after === main, `le modèle principal a changé alors que le test a échoué : ${after}`);
      }

      // Choix d'un modèle vocal fonctionnel, puis retrait.
      const flash = p.locator(".model-row", { hasText: "glm-5.3-flash" });
      if ((await flash.count()) > 0) {
        await flash.locator("button", { hasText: "Vocal" }).click();
        // Attendre que CE modèle devienne vocal : un modèle vocal déjà choisi
        // satisfaisait « .model-row.is-voice » tout de suite (échec intermittent).
        await p.waitForFunction(
          () => [...document.querySelectorAll(".model-row.is-voice")].some((r) => r.textContent.includes("glm-5.3-flash")),
          null,
          { timeout: 90000 },
        );
        const current = (await p.locator(".model-current").textContent()) ?? "";
        expect(current.includes("glm-5.3-flash"), `modèle vocal non affiché : ${current}`);
        // La valeur réellement enregistrée doit être l'identifiant court, jamais « fournisseur/modèle ».
        const saved = await p.evaluate(async () => (await window.__TAURI_INTERNALS__.invoke("status")).llm.voice_model);
        expect(saved === "glm-5.3-flash", `modèle vocal enregistré : « ${saved} »`);
        await p.locator(".model-current button", { hasText: "Retirer" }).click();
        await p.waitForSelector(".model-row.is-voice", { state: "detached", timeout: 15000 });
      }
    } finally {
      // La suite ne doit jamais laisser un modèle vocal modifié dans ta
      // configuration : on remet exactement celui qui était choisi.
      await p.evaluate((model) => window.__TAURI_INTERNALS__.invoke("set_llm_model", { role: "voice", model }), savedVoice);
      const restored = await p.evaluate(async () => (await window.__TAURI_INTERNALS__.invoke("status")).llm.voice_model ?? "");
      if (restored !== savedVoice) throw new Error(`modèle vocal non restauré : « ${restored} » au lieu de « ${savedVoice} »`);
    }
    return `${total} modèles, principal « ${main} » testé`;
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
    await nav("Historique");
    await p.waitForSelector(".list-row", { timeout: 5000 });
    await p.locator(".list-row").first().locator("button", { hasText: "Ouvrir" }).click();
    await p.waitForTimeout(1000);
    const title = await p.locator(".topbar h1").textContent();
    const bubbles = await p.locator(".stream .bubble").count();
    expect(title === "Chat" && bubbles > 0, `${title}, ${bubbles} bulles`);
    return `${bubbles} bulles`;
  });

  // Historique groupé par projet + recherche instantanée + Ctrl+K.
  await step("Historique : projets groupés, recherche, Ctrl+K", async () => {
    const repoName = "jimmy-agent-personnel";
    await nav("Historique");
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

  await step("Mémoire : libellés traduits", async () => {
    await nav("Mémoire");
    await p.waitForTimeout(800);
    const tags = await p.$$eval(".tag", (els) => els.map((e) => e.textContent));
    expect(!tags.some((t) => /semantic|procedural|episodic/.test(t)), `tags : ${tags.join(", ")}`);
    return tags.join(", ") || "aucun souvenir";
  });

  await step("Skills : sous-onglet des serveurs MCP", async () => {
    await nav("Skills");
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

  await step("Diagnostic : toutes les vérifications passent", async () => {
    await nav("Diagnostic");
    await p.waitForSelector(".ok-note, .warn-note", { timeout: 15000 });
    return await p.locator(".ok-note, .warn-note").first().textContent();
  });

  await nav("Voix");
  await p.screenshot({ path: path.join(OUT, "suite-voix.png") });
  await nav("Chat");
  await p.screenshot({ path: path.join(OUT, "suite-chat.png") });

  await step("Aucune erreur JavaScript", async () => {
    expect(errors.length === 0, errors.join(" | "));
  });

  console.log(results.join("\n"));
  await browser.close();
  process.exit(results.some((r) => r.startsWith("ÉCHEC")) ? 1 : 0);
})();
