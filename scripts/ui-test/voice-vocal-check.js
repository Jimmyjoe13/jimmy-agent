// Vérification ciblée : choix du modèle vocal depuis l'onglet Voix.
// Usage : Jimmy relancé avec --remote-debugging-port=9222, puis
// `node scripts/ui-test/voice-vocal-check.js`. Ne touche pas aux sessions.
const { chromium } = require("playwright-core");

const expect = (cond, msg) => {
  if (!cond) throw new Error(msg);
};

(async () => {
  const browser = await chromium.connectOverCDP("http://127.0.0.1:9222");
  const p = browser.contexts()[0].pages().find((pg) => pg.url().includes("tauri.localhost"));
  if (!p) throw new Error("page Tauri introuvable");
  const errors = [];
  p.on("pageerror", (e) => errors.push(e.message));

  const saved = await p.evaluate(async () => (await window.__TAURI_INTERNALS__.invoke("status")).llm);
  const savedVoice = saved.voice_model ?? "";
  const savedVoiceProvider = saved.voice_provider || saved.provider || "opencode";
  console.log(`fournisseur : ${saved.provider}, vocal initial : « ${savedVoice} »`);

  try {
    await p.locator(".nav-item", { hasText: "Voix" }).click();
    await p.waitForSelector(".models-panel .model-row", { timeout: 40000 });
    // Première ligne non vocale : le choix passe par un vrai test du modèle.
    const target = p.locator(".models-panel .model-row:not(.is-voice)").first();
    const id = (await target.locator("code").first().textContent()) ?? "";
    expect(id.length > 0, "identifiant illisible");
    console.log(`cible : ${id}`);
    await target.locator("button", { hasText: "Vocal" }).click();
    await p.waitForFunction(
      (wanted) => [...document.querySelectorAll(".models-panel .model-row.is-voice")].some((r) => r.textContent.includes(wanted)),
      id,
      { timeout: 120000 },
    );
    const current = (await p.locator(".models-panel .model-current").textContent()) ?? "";
    expect(current.includes(id), `modèle vocal non affiché : ${current}`);
    const stored = await p.evaluate(async () => (await window.__TAURI_INTERNALS__.invoke("status")).llm.voice_model);
    expect(stored === id, `modèle vocal enregistré : « ${stored} » (attendu « ${id} »)`);
    // Le champ identifiant de la carte suit le choix de la bibliothèque.
    const field = await p.locator(".field-row", { hasText: "Modèle vocal" }).locator("input").inputValue();
    expect(field === id, `champ modèle vocal : « ${field} » (attendu « ${id} »)`);
    console.log("choix vocal OK, champ synchronisé");
    await p.locator(".models-panel .model-current button", { hasText: "Retirer" }).click();
    await p.waitForSelector(".models-panel .model-row.is-voice", { state: "detached", timeout: 15000 });
    console.log("retrait OK");
  } finally {
    await p.evaluate(
      ([model, provider]) => window.__TAURI_INTERNALS__.invoke("set_llm_model", { role: "voice", model, provider: model ? provider : null }),
      [savedVoice, savedVoiceProvider],
    );
    const restored = await p.evaluate(async () => (await window.__TAURI_INTERNALS__.invoke("status")).llm);
    if ((restored.voice_model ?? "") !== savedVoice) throw new Error(`modèle vocal non restauré : « ${restored.voice_model} »`);
    console.log(`restauré : « ${restored.voice_model ?? "(principal)"} »`);
  }
  expect(errors.length === 0, `erreurs JS : ${errors.join(" | ")}`);
  console.log("VOIX-VOCAL-CHECK OK, 0 erreur JS");
  await browser.close();
})().catch((e) => {
  console.error(`VOIX-VOCAL-CHECK ÉCHEC — ${e.message}`);
  process.exit(1);
});
