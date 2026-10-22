/**
 * Explorateur de fichiers du Chat (panneau latéral).
 *
 * Arborescence du projet de la conversation, chargée à la demande (un dossier
 * n'est lu que quand on le déplie). Un clic sur un fichier en montre un
 * aperçu ; « Ouvrir » le confie à l'application par défaut de Windows
 * (un exécutable est seulement montré dans l'Explorateur, jamais lancé).
 */
import { api, type FsEntry } from "../api";
import { locale, t } from "../i18n";
import { attempt, guard, h, mount, toast } from "../ui";

export interface ExplorerPanel {
  element: HTMLElement;
  /** Racine affichée (projet de la conversation, ou dossier par défaut). */
  setRoot: (path: string, label: string) => void;
}

/** Taille lisible (o, Ko, Mo). */
function formatSize(bytes: number): string {
  if (bytes < 1024) return t("explorer.bytes", { n: bytes });
  if (bytes < 1024 * 1024) return t("explorer.kb", { n: Math.round(bytes / 1024) });
  return t("explorer.mb", { n: (bytes / (1024 * 1024)).toLocaleString(locale(), { maximumFractionDigits: 1 }) });
}

export function explorerPanel(): ExplorerPanel {
  let root = "";
  let selected: HTMLElement | null = null;

  const title = h("strong", { class: "explorer-title" }, t("explorer.files"));
  const tree = h("div", { class: "explorer-tree", role: "tree", "aria-label": t("explorer.filesLabel") });
  const preview = h("div", { class: "explorer-preview", hidden: true });

  /** Lignes d'un dossier, insérées dans `container`, au niveau `depth`. */
  async function fill(container: HTMLElement, path: string, depth: number) {
    const listing = await guard(() => api.fsList(path), t("guard.explorer"));
    if (!listing) {
      mount(container);
      return;
    }
    mount(container);
    if (listing.entries.length === 0) {
      container.append(h("div", { class: "tree-empty", style: `padding-left:${8 + depth * 14}px` }, t("explorer.emptyFolder")));
    }
    for (const entry of listing.entries) container.append(row(entry, depth));
    if (listing.truncated) {
      container.append(h("div", { class: "tree-empty", style: `padding-left:${8 + depth * 14}px` }, t("explorer.truncated")));
    }
  }

  function row(entry: FsEntry, depth: number): HTMLElement {
    const caret = h("span", { class: "tree-caret" }, entry.dir ? "▸" : "");
    const line = h(
      "div",
      {
        class: `tree-row${entry.dir ? " dir" : ""}`,
        role: "treeitem",
        title: entry.path,
        style: `padding-left:${8 + depth * 14}px`,
      },
      caret,
      h("span", { class: "tree-name" }, entry.name),
    );
    if (!entry.dir) {
      line.addEventListener("click", () => void show(entry, line));
      line.addEventListener("dblclick", () => void open(entry.path));
      return line;
    }
    // Dossier : déplié à la demande, une seule lecture par dépliage.
    const children = h("div", { class: "tree-children", hidden: true });
    line.addEventListener("click", () => {
      const opening = children.hidden;
      children.hidden = !opening;
      caret.textContent = opening ? "▾" : "▸";
      if (opening) void fill(children, entry.path, depth + 1);
    });
    return h("div", {}, line, children);
  }

  async function open(path: string) {
    const how = await guard(() => api.fsOpen(path), t("guard.opening"));
    if (how === "revealed") toast(t("explorer.exeRevealed"));
  }

  async function show(entry: FsEntry, line: HTMLElement) {
    selected?.classList.remove("selected");
    selected = line;
    line.classList.add("selected");
    const data = await guard(() => api.fsPreview(entry.path), t("guard.preview"));
    if (!data) return;
    preview.hidden = false;
    mount(
      preview,
      h(
        "div",
        { class: "preview-head" },
        h("strong", { title: data.path }, entry.name),
        h("span", { class: "row-meta" }, formatSize(data.size)),
      ),
      h(
        "div",
        { class: "row" },
        h(
          "button",
          { class: "small", onclick: () => void open(entry.path) },
          data.executable ? t("explorer.reveal") : t("explorer.open"),
        ),
        data.executable
          ? null
          : h("button", { class: "ghost small", onclick: () => void attempt(() => api.fsReveal(entry.path), t("explorer.windowsExplorer")) }, t("explorer.inExplorer")),
        h("button", { class: "ghost small", title: t("explorer.closePreview"), onclick: () => (preview.hidden = true) }, "×"),
      ),
      data.binary
        ? h("p", { class: "note" }, t("explorer.binary"))
        : h("pre", { class: "preview-text" }, data.text || t("explorer.emptyFile")),
      data.truncated ? h("p", { class: "row-meta" }, t("explorer.previewLimit")) : null,
    );
  }

  const element = h(
    "aside",
    { class: "explorer", "aria-label": t("explorer.label") },
    h(
      "div",
      { class: "explorer-head" },
      title,
      h("button", { class: "ghost small", title: t("explorer.refresh"), onclick: () => root && void fill(tree, root, 0) }, "↻"),
      h(
        "button",
        { class: "ghost small", title: t("explorer.openInWindows"), onclick: () => root && void open(root) },
        t("explorer.windowsExplorer"),
      ),
    ),
    tree,
    preview,
  );

  return {
    element,
    setRoot(path: string, label: string) {
      if (path === root) return;
      root = path;
      title.textContent = label;
      title.setAttribute("title", path);
      preview.hidden = true;
      void fill(tree, path, 0);
    },
  };
}
