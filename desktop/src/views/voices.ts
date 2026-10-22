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
import { t } from "../i18n";

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
    placeholder: t("voices.searchPlaceholder"),
    "aria-label": t("voices.searchLabel"),
    onkeydown: (event: Event) => {
      if ((event as KeyboardEvent).key === "Enter") void search();
    },
  }) as HTMLInputElement;
  const searchButton = h("button", { class: "primary", onclick: () => void search() }, t("voices.search"));

  const all = () => [...presets, ...library];
  const isPreset = (id: string) => presets.some((v) => v.id === id);

  async function load() {
    const data = await guard(() => api.ttsVoices(), t("guard.voice"));
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
    await attempt(() => api.ttsPreview(t("voices.previewText"), voice.id), t("guard.voicePreview"));
    playing = null;
    render();
  }

  async function choose(voice: VoiceInfo) {
    if (!(await attempt(() => api.ttsSetVoice(voice), t("guard.voiceChoice")))) return;
    current = voice.id;
    if (!isPreset(voice.id) && !library.some((v) => v.id === voice.id)) library = [...library, voice];
    hooks.onApplied(current, library);
    render();
    toast(t("voices.chosen", { label: voice.label }));
  }

  async function remove(voice: VoiceInfo) {
    const next = await guard(() => api.ttsRemoveVoice(voice.id), t("guard.voiceRemove"));
    if (next === undefined) return;
    library = library.filter((v) => v.id !== voice.id);
    current = next;
    hooks.onApplied(current, library);
    render();
    toast(t("voices.removed", { label: voice.label }));
  }

  async function search() {
    const query = searchInput.value.trim();
    if (!query) return;
    searchButton.setAttribute("disabled", "");
    mount(resultsBox, h("p", { class: "note" }, t("voices.searching")));
    const found = await guard(() => api.ttsSearchVoices(query), t("guard.fishCatalog"));
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
          active ? h("span", { class: "badge main", title: t("voices.currentHint") }, t("voices.currentBadge")) : null,
          isPreset(voice.id) ? h("span", { class: "badge", title: t("voices.presetHint") }, t("voices.presetBadge")) : null,
          voice.uses ? h("span", { class: "row-meta", title: t("voices.usesHint") }, t("voices.uses", { count: formatUses(voice.uses) })) : null,
        ),
        voice.description ? h("p", { class: "model-desc", title: voice.description }, voice.description) : null,
      ),
      h(
        "div",
        { class: "model-actions" },
        h(
          "button",
          { class: "ghost small", disabled: playing !== null, onclick: () => void preview(voice) },
          busy ? t("voices.playing") : t("voices.listen"),
        ),
        active ? null : h("button", { class: "small", onclick: () => void choose(voice) }, t("voices.choose")),
        inLibrary && !isPreset(voice.id)
          ? h("button", { class: "ghost small", title: t("voices.removeHint"), onclick: () => void remove(voice) }, t("voices.remove"))
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
        h("span", { class: "model-role" }, t("voices.role")),
        h("strong", {}, voice?.label ?? t("voices.custom")),
        voice?.description ? h("span", { class: "note" }, ` · ${voice.description}`) : h("code", {}, current),
      ),
    );
    mount(list, ...all().map((v) => row(v, true)));
    renderResults();
  }

  function renderResults() {
    if (results.length === 0) {
      mount(resultsBox, searchInput.value.trim() ? h("p", { class: "note" }, t("voices.noneFound")) : null);
      return;
    }
    mount(resultsBox, ...results.map((v) => row(v, all().some((x) => x.id === v.id))));
  }

  const panel = h(
    "div",
    { class: "voices-panel" },
    currentBox,
    h("p", { class: "row-meta" }, t("voices.library")),
    list,
    h("p", { class: "row-meta" }, t("voices.addFromCatalog")),
    h("div", { class: "voice-search" }, searchInput, searchButton),
    resultsBox,
  );
  // Quand le moteur de synthèse change (onglet Voix), la liste des voix
  // prédéfinies change aussi.
  window.addEventListener("jimmy-tts-provider-changed", () => void load());
  void load();
  return panel;
}

function formatUses(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1).replace(".", ",")} M`;
  if (n >= 1_000) return `${Math.round(n / 1_000)} k`;
  return String(n);
}
