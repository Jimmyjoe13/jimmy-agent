// jimy-agent.me — démo du hero, collection de skins, apparitions au défilement.
// Aucun appel réseau, aucune dépendance.
(() => {
  "use strict";
  const $ = (s, r = document) => r.querySelector(s);
  const $$ = (s, r = document) => [...r.querySelectorAll(s)];
  const calm = matchMedia("(prefers-reduced-motion: reduce)").matches;
  const wait = (ms) => new Promise((ok) => setTimeout(ok, calm ? Math.min(ms, 400) : ms));

  // ── Fiche de chaque skin (nom d'appel, compagnon, lueur de la scène) ──────
  const SKINS = {
    renard:   { nom: "Renard",   wake: "Jimy", buddy: "le lapin", prix: "Offert", badge: "free", glow: "rgba(255,106,77,.32)",
                desc: "Le Jimy d'origine : pelage roux, écharpe dorée." },
    arctique: { nom: "Arctique", wake: "Jimy", buddy: "le lapin", prix: "Offert", badge: "free", glow: "rgba(42,212,247,.26)",
                desc: "Pelage blanc et écharpe turquoise, pour les bureaux minimalistes." },
    fennec:   { nom: "Fennec",   wake: "Jimy", buddy: "le lapin", prix: "Offert", badge: "free", glow: "rgba(255,196,92,.26)",
                desc: "Grandes oreilles et pelage sable." },
    ours:     { nom: "Ours",     wake: "Jimy", buddy: "le lapin", prix: "Offert", badge: "free", glow: "rgba(190,120,70,.3)",
                desc: "Plus trapu, petites oreilles rondes, pelage brun." },
    robot:    { nom: "Robot",    wake: "Jimy", buddy: "le lapin", prix: "Offert", badge: "free", glow: "rgba(160,170,200,.24)",
                desc: "Gris métal, sans un poil." },
    neon:     { nom: "Néon",     wake: "Néon", buddy: "le lapin néon", prix: "4,90 € · bientôt", badge: "premium", glow: "rgba(255,120,40,.38)",
                desc: "Corps sombre, contour orange épais : il se voit sur n'importe quel fond d'écran." },
    jeva:     { nom: "JEVA",     wake: "JEVA", buddy: "la sonde", prix: "Bientôt", badge: "premium", glow: "rgba(90,140,255,.36)",
                desc: "Androïde chromé à l'œil unique, en lévitation, entouré de câbles lumineux." },
    ark:      { nom: "ARK",      wake: "ARK", buddy: "la sonde ARK", prix: "Bientôt", badge: "premium", glow: "rgba(110,255,120,.3)",
                desc: "Sphère mécanique à l'œil vert fendu, douze tentacules à pinces animés par ressorts." },
  };
  const src = (k) => `/assets/skins/${k}.webp`;

  // Change une image avec un court fondu (classe .swap posée par le CSS).
  async function swapImg(img, url) {
    if (img.getAttribute("src") === url) return;
    img.classList.add("swap");
    await wait(220);
    img.src = url;
    await img.decode().catch(() => {});
    img.classList.remove("swap");
  }

  // ── Hero : scénarios joués en boucle sur le faux bureau ───────────────────
  // {W} = nom d'appel du skin courant (« Jimy », « JEVA »…).
  const SCENES = [
    { said: "{W}, range mes téléchargements.",
      task: ["› liste Téléchargements : 142 fichiers", "› crée Images, PDF, Archives", ["ok", "✓ 34 fichiers déplacés"]],
      reply: "C'est fait : 34 fichiers rangés par type. Je n'ai rien supprimé." },
    { said: "{W}, regarde mon écran : pourquoi ça plante ?",
      task: ["› capture de la fenêtre active", "› lit app.py, ligne 42", ["ok", "✓ cause trouvée"]],
      reply: "Ton script cherche config.json dans le mauvais dossier. Je corrige ?" },
    { said: "{W}, trouve-moi un train pour Lyon vendredi matin.",
      task: ["› navigateur : recherche des horaires", "› 3 trains avant 10 h", ["wait", "⏸ réservation : j'attends ton accord"]],
      reply: "J'en ai trois avant 10 h. Je réserve celui de 7 h 56 ?" },
  ];
  let heroSkin = "renard";

  const bubble = $("#bubble"), said = $("#said"), saidText = $("#said-text"), task = $("#task");

  // Écrit le texte lettre par lettre dans la bulle, avec un curseur.
  async function type(el, text) {
    el.innerHTML = '<span></span><i class="caret"></i>';
    const span = el.firstChild;
    for (const ch of text) {
      span.textContent += ch;
      await wait(ch === " " ? 18 : 28);
    }
    el.lastChild.remove();
  }

  async function play() {
    if (!bubble) return;
    for (let i = 0; ; i = (i + 1) % SCENES.length) {
      const s = SCENES[i];
      // 1. L'utilisateur parle.
      saidText.textContent = s.said.replace("{W}", SKINS[heroSkin].wake);
      said.classList.remove("off");
      await wait(1300);
      // 2. Jimy répond « Oui ? » puis travaille (fenêtre de tâche).
      bubble.textContent = "Oui ?";
      bubble.classList.remove("off");
      await wait(900);
      bubble.classList.add("off");
      said.classList.add("off");
      task.innerHTML = '<div class="bar"><span></span><span></span><span></span></div>';
      task.classList.add("on");
      for (const line of s.task) {
        await wait(650);
        const d = document.createElement("div");
        if (Array.isArray(line)) { d.className = line[0]; d.textContent = line[1]; } else d.textContent = line;
        task.appendChild(d);
      }
      // 3. Réponse dans la bulle.
      await wait(500);
      bubble.classList.remove("off");
      await type(bubble, s.reply);
      await wait(3200);
      bubble.classList.add("off");
      task.classList.remove("on");
      await wait(900);
    }
  }

  // Sélecteur de skin du hero.
  $$(".chip[data-skin]").forEach((chip) => chip.addEventListener("click", () => {
    heroSkin = chip.dataset.skin;
    $$(".chip[data-skin]").forEach((c) => c.setAttribute("aria-pressed", String(c === chip)));
    swapImg($("#avatar"), src(heroSkin));
  }));

  // Horloge de la barre des tâches : l'heure réelle du visiteur.
  const clock = $("#clock");
  const tick = () => { if (clock) clock.textContent = new Date().toLocaleTimeString("fr-FR", { hour: "2-digit", minute: "2-digit" }); };
  tick(); setInterval(tick, 30000);

  // ── Collection : fiche du skin sélectionné ────────────────────────────────
  function show(key) {
    const k = SKINS[key];
    $$(".thumb[data-show]").forEach((t) => t.setAttribute("aria-pressed", String(t.dataset.show === key)));
    swapImg($("#stage-img"), src(key));
    $("#stage-img").alt = `Skin ${k.nom}`;
    $("#stage").style.setProperty("--glow", k.glow);
    const badge = $("#stage-badge");
    badge.className = `badge ${k.badge}`;
    badge.textContent = k.badge === "free" ? "Offert" : "Premium";
    $("#skin-name").textContent = k.nom;
    $("#skin-desc").textContent = k.desc;
    $("#skin-wake").textContent = `« ${k.wake} »`;
    $("#skin-buddy").textContent = k.buddy;
    $("#skin-price").textContent = k.prix;
  }
  $$(".thumb[data-show]").forEach((t) => t.addEventListener("click", () => show(t.dataset.show)));

  // ── Apparitions au défilement ─────────────────────────────────────────────
  const io = new IntersectionObserver((entries) => {
    for (const e of entries) if (e.isIntersecting) { e.target.classList.add("in"); io.unobserve(e.target); }
  }, { rootMargin: "0px 0px -8% 0px" });
  $$(".reveal").forEach((el) => io.observe(el));

  // Précharge les skins de la démo pour des changements instantanés.
  ["neon", "jeva", "ark"].forEach((k) => { const i = new Image(); i.src = src(k); });

  play();
})();
