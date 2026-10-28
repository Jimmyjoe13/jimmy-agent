/**
 * Âme de Jimy (Paramètres → Âme) : `data/SOUL.md`, son caractère et ses
 * leçons apprises, injectés en tête de chaque prompt.
 *
 * Jimy la fait évoluer lui-même (outil `soul_update`, revue périodique) :
 * ce panneau sert à relire ce qu'il a changé, corriger, annuler la dernière
 * modification ou revenir à l'âme d'origine. Mêmes garde-fous côté Rust que
 * pour Jimy (taille bornée, aucun secret, copie `.bak`).
 *
 * Classes propres à cette vue (`soul-editor`, `soul-count`, `soul-changes`) :
 * les réutiliser depuis une autre fausse ses tests (piège 63).
 */
import { api, type SoulInfo } from "../api";
import { attempt, guard, h, mount, toast } from "../ui";
import { t } from "../i18n";

/** Carte locale : même style que `card` de settings.ts, sans import circulaire. */
function card(title: string, ...children: (Node | string | null | false)[]): HTMLElement {
  return h("section", { class: "card" }, h("h3", {}, title), ...children);
}

export function soulPanel(): HTMLElement {
  const root = h("div", {});
  let info: SoulInfo | null = null;

  const editor = h("textarea", { class: "soul-editor", rows: "18", spellcheck: "false" }) as HTMLTextAreaElement;
  const count = h("span", { class: "soul-count note" });
  const changes = h("ul", { class: "soul-changes" });
  const undoButton = h("button", { class: "ghost", onclick: () => void undo() }, t("soul.undo")) as HTMLButtonElement;

  /** Compteur « 1 234 / 2 500 », en alerte au-delà de la borne. */
  function updateCount() {
    const max = info?.max ?? 2500;
    const n = [...editor.value].length;
    count.textContent = t("soul.count", { n: String(n), max: String(max) });
    count.classList.toggle("over", n > max);
  }
  editor.addEventListener("input", updateCount);

  async function load() {
    const loaded = await guard(() => api.soulGet(), t("guard.soul"));
    if (!loaded) return;
    info = loaded;
    editor.value = loaded.content;
    undoButton.disabled = !loaded.canUndo;
    mount(
      changes,
      ...(loaded.changes.length
        ? loaded.changes.map((line) => h("li", {}, line))
        : [h("li", { class: "note" }, t("soul.noChanges"))]),
    );
    updateCount();
  }

  async function save() {
    if (!(await attempt(() => api.soulSave(editor.value), t("guard.soul")))) return;
    toast(t("soul.saved"));
    await load();
  }

  async function undo() {
    if (!(await attempt(() => api.soulUndo(), t("guard.soul")))) return;
    toast(t("soul.undone"));
    await load();
  }

  async function reset() {
    if (!confirm(t("soul.resetConfirm"))) return;
    if (!(await attempt(() => api.soulReset(), t("guard.soul")))) return;
    toast(t("soul.resetDone"));
    await load();
  }

  mount(
    root,
    card(
      t("soul.title"),
      h("p", { class: "note" }, t("soul.note")),
      editor,
      h(
        "div",
        { class: "row" },
        count,
        h("button", { class: "ghost", onclick: () => void reset() }, t("soul.reset")),
        undoButton,
        h("button", { class: "primary", onclick: () => void save() }, t("soul.save")),
      ),
    ),
    card(t("soul.changesTitle"), changes),
  );
  void load();
  return root;
}
