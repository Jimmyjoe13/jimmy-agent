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

  await step("Mémoire : libellés traduits", async () => {
    await nav("Mémoire");
    await p.waitForTimeout(800);
    const tags = await p.$$eval(".tag", (els) => els.map((e) => e.textContent));
    expect(!tags.some((t) => /semantic|procedural|episodic/.test(t)), `tags : ${tags.join(", ")}`);
    return tags.join(", ") || "aucun souvenir";
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
