/**
 * Bibliothèque de voix de Jimy (Fish Audio).
 *
 * Trois voix prédéfinies (« Le narrateur » par défaut), plus celles que
 * l'utilisateur ajoute depuis le catalogue public de Fish Audio. Une voix
 * s'écoute avant d'être choisie ; une voix choisie depuis la recherche est
 * gardée dans la bibliothèque (`tts.library`).
 */
import { api, type VoiceInfo } from "../api";
import { attempt, guard, h, mount, toast } from "../ui";

/** Phrase lue pour un essai : courte, avec le nom de Jimy. */
const PREVIEW_TEXT = "Bonjour, je suis Jimy. Voici ma voix, pour lire tes réponses.";

export interface VoicesPanelHooks {
  /**
   * Appelé après un changement enregistré côté Rust : la vue Paramètres met à
   * jour sa copie, sinon « Enregistrer » remettrait l'ancienne voix.
   */
  onApplied: (voiceId: string, library: VoiceInfo[]) => void;
}

export function voicesPanel(hooks: VoicesPanelHooks): HTMLElement {
  let current = "";
  let presets: VoiceInfo[] = [];
  let library: VoiceInfo[] = [];
  let results: VoiceInfo[] = [];
  // Une seule lecture d'essai à la fois (elle passe par les haut-parleurs).
  let playing: string | null = null;

  const currentBox = h("div", { class: "voice-current" });
  const list = h("div", { class: "voice-list", "aria-live": "polite" });
  const resultsBox = h("div", { class: "voice-list", "aria-live": "polite" });
  const searchInput = h("input", {
    class: "field",
    type: "search",
    placeholder: "Chercher une voix française (narrateur, calme, grave…)",
    "aria-label": "Chercher une voix dans le catalogue Fish Audio",
    onkeydown: (event: Event) => {
      if ((event as KeyboardEvent).key === "Enter") void search();
    },
  }) as HTMLInputElement;
  const searchButton = h("button", { class: "primary", onclick: () => void search() }, "Chercher");

  const all = () => [...presets, ...library];
  const isPreset = (id: string) => presets.some((v) => v.id === id);

  async function load() {
    const data = await guard(() => api.ttsVoices(), "voix");
    if (!data) return;
    current = data.current;
    presets = data.presets;
    library = data.library;
    render();
  }

  /** Lecture d'essai d'une voix, sans la choisir. */
  async function preview(voice: VoiceInfo) {
    if (playing) return;
    playing = voice.id;
    render();
    await attempt(() => api.ttsPreview(PREVIEW_TEXT, voice.id), "essai de voix");
    playing = null;
    render();
  }

  async function choose(voice: VoiceInfo) {
    if (!(await attempt(() => api.ttsSetVoice(voice), "choix de la voix"))) return;
    current = voice.id;
    if (!isPreset(voice.id) && !library.some((v) => v.id === voice.id)) library = [...library, voice];
    hooks.onApplied(current, library);
    render();
    toast(`Voix de Jimy : ${voice.label}`);
  }

  async function remove(voice: VoiceInfo) {
    const next = await guard(() => api.ttsRemoveVoice(voice.id), "retrait de la voix");
    if (next === undefined) return;
    library = library.filter((v) => v.id !== voice.id);
    current = next;
    hooks.onApplied(current, library);
    render();
    toast(`« ${voice.label} » retirée de la bibliothèque`);
  }

  async function search() {
    const query = searchInput.value.trim();
    if (!query) return;
    searchButton.setAttribute("disabled", "");
    mount(resultsBox, h("p", { class: "note" }, "Recherche dans le catalogue Fish Audio…"));
    const found = await guard(() => api.ttsSearchVoices(query), "catalogue Fish Audio");
    searchButton.removeAttribute("disabled");
    results = found ?? [];
    renderResults();
  }

  function row(voice: VoiceInfo, inLibrary: boolean): HTMLElement {
    const active = voice.id === current;
    const busy = playing === voice.id;
    return h(
      "div",
      { class: `voice-row${active ? " is-main" : ""}` },
      h(
        "div",
        { class: "model-id" },
        h(
          "strong",
          {},
          voice.label,
          active ? h("span", { class: "badge main", title: "Voix utilisée par Jimy" }, "actuelle") : null,
          isPreset(voice.id) ? h("span", { class: "badge", title: "Voix proposée par défaut" }, "prédéfinie") : null,
          voice.uses ? h("span", { class: "row-meta", title: "Utilisations sur Fish Audio" }, `${formatUses(voice.uses)} lectures`) : null,
        ),
        voice.description ? h("p", { class: "model-desc", title: voice.description }, voice.description) : null,
      ),
      h(
        "div",
        { class: "model-actions" },
        h(
          "button",
          { class: "ghost small", disabled: playing !== null, onclick: () => void preview(voice) },
          busy ? "Lecture…" : "Écouter",
        ),
        active ? null : h("button", { class: "small", onclick: () => void choose(voice) }, "Choisir"),
        inLibrary && !isPreset(voice.id)
          ? h("button", { class: "ghost small", title: "Retirer de la bibliothèque", onclick: () => void remove(voice) }, "Retirer")
          : null,
      ),
    );
  }

  function render() {
    const voice = all().find((v) => v.id === current);
    mount(
      currentBox,
      h(
        "div",
        {},
        h("span", { class: "model-role" }, "Voix"),
        h("strong", {}, voice?.label ?? "voix personnalisée"),
        voice?.description ? h("span", { class: "note" }, ` · ${voice.description}`) : h("code", {}, current),
      ),
    );
    mount(list, ...all().map((v) => row(v, true)));
    renderResults();
  }

  function renderResults() {
    if (results.length === 0) {
      mount(resultsBox, searchInput.value.trim() ? h("p", { class: "note" }, "Aucune voix trouvée.") : null);
      return;
    }
    mount(resultsBox, ...results.map((v) => row(v, all().some((x) => x.id === v.id))));
  }

  const panel = h(
    "div",
    { class: "voices-panel" },
    currentBox,
    h("p", { class: "row-meta" }, "Ta bibliothèque"),
    list,
    h("p", { class: "row-meta" }, "Ajouter une voix depuis le catalogue Fish Audio"),
    h("div", { class: "voice-search" }, searchInput, searchButton),
    resultsBox,
  );
  void load();
  return panel;
}

function formatUses(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1).replace(".", ",")} M`;
  if (n >= 1_000) return `${Math.round(n / 1_000)} k`;
  return String(n);
}
