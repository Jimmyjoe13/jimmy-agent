// Supprime les sessions créées par la suite de tests (message « banane »).
const { chromium } = require("playwright-core");
(async () => {
  const browser = await chromium.connectOverCDP("http://127.0.0.1:9222");
  const p = browser.contexts()[0].pages().find((pg) => pg.url().includes("tauri.localhost"));
  const removed = await p.evaluate(async () => {
    const invoke = window.__TAURI_INTERNALS__.invoke;
    const sessions = await invoke("sessions");
    const out = [];
    for (const s of sessions) {
      const messages = await invoke("session_messages", { sessionId: s.id });
      if (messages.some((m) => m.content.includes("Réponds uniquement par le mot : banane"))) {
        await invoke("delete_session", { sessionId: s.id });
        out.push(s.title);
      }
    }
    return out;
  });
  console.log("sessions de test supprimées :", removed);
  await browser.close();
})();
