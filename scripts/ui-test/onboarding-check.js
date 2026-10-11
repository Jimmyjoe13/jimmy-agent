// Parcourt l'écran d'accueil d'une installation neuve (sans le terminer) :
// vérifie l'étape de la clé du modèle et l'étape des accès.
// Usage : Jimy installé lancé avec --remote-debugging-port=9222, puis
//   node scripts/ui-test/onboarding-check.js <dossier_captures>
const { chromium } = require("playwright-core");
(async () => {
  const out = process.argv[2] || ".";
  const browser = await chromium.connectOverCDP("http://127.0.0.1:9222");
  const p = browser.contexts()[0].pages()[0];
  // Repart de la première étape.
  await p.reload();
  await p.waitForSelector(".onboarding-card", { timeout: 15000 });
  const seen = [];
  for (let i = 0; i < 8; i++) {
    const title = await p.locator(".onboarding-card h2").textContent();
    seen.push(title);
    if (title.includes("modèle")) {
      const key = await p.locator('.onboarding-card input[type="password"]').count();
      console.log(`clé : champ ${key === 1 ? "présent" : "ABSENT"}`);
      await p.screenshot({ path: `${out}/accueil-cle.png` });
    }
    if (title.includes("Ce que je peux faire")) {
      const boxes = await p.locator('.onboarding-card input[type="checkbox"]').evaluateAll((els) => els.map((e) => e.checked));
      console.log(`accès : ${boxes.length} cases, cochées ${JSON.stringify(boxes)}`);
      await p.screenshot({ path: `${out}/accueil-acces.png` });
    }
    if (i < 7) await p.locator(".onboarding-footer button.primary").click();
    await p.waitForTimeout(250);
  }
  console.log("étapes : " + seen.join(" | "));
  await browser.close();
})().catch((e) => { console.error(e); process.exit(1); });
