/**
 * Explorateur de fichiers du Chat (panneau latéral).
 *
 * Arborescence du projet de la conversation, chargée à la demande (un dossier
 * n'est lu que quand on le déplie). Un clic sur un fichier en montre un
 * aperçu ; « Ouvrir » le confie à l'application par défaut de Windows
 * (un exécutable est seulement montré dans l'Explorateur, jamais lancé).
 */
import { api, type FsEntry } from "../api";
import { attempt, guard, h, mount, toast } from "../ui";

export interface ExplorerPanel {
  element: HTMLElement;
  /** Racine affichée (projet de la conversation, ou dossier par défaut). */
  setRoot: (path: string, label: string) => void;
}

/** Taille lisible (o, Ko, Mo). */
function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} o`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} Ko`;
  return `${(bytes / (1024 * 1024)).toFixed(1).replace(".", ",")} Mo`;
}

export function explorerPanel(): ExplorerPanel {
  let root = "";
  let selected: HTMLElement | null = null;

  const title = h("strong", { class: "explorer-title" }, "Fichiers");
  const tree = h("div", { class: "explorer-tree", role: "tree", "aria-label": "Fichiers du projet" });
  const preview = h("div", { class: "explorer-preview", hidden: true });

  /** Lignes d'un dossier, insérées dans `container`, au niveau `depth`. */
  async function fill(container: HTMLElement, path: string, depth: number) {
    const listing = await guard(() => api.fsList(path), "explorateur");
    if (!listing) {
      mount(container);
      return;
    }
    mount(container);
    if (listing.entries.length === 0) {
      container.append(h("div", { class: "tree-empty", style: `padding-left:${8 + depth * 14}px` }, "dossier vide"));
    }
    for (const entry of listing.entries) container.append(row(entry, depth));
    if (listing.truncated) {
      container.append(h("div", { class: "tree-empty", style: `padding-left:${8 + depth * 14}px` }, "… liste tronquée (500 entrées)"));
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
    const how = await guard(() => api.fsOpen(path), "ouverture");
    if (how === "revealed") toast("Fichier exécutable : montré dans l'Explorateur, pas lancé.");
  }

  async function show(entry: FsEntry, line: HTMLElement) {
    selected?.classList.remove("selected");
    selected = line;
    line.classList.add("selected");
    const data = await guard(() => api.fsPreview(entry.path), "aperçu");
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
          data.executable ? "Afficher dans l'Explorateur" : "Ouvrir",
        ),
        data.executable
          ? null
          : h("button", { class: "ghost small", onclick: () => void attempt(() => api.fsReveal(entry.path), "Explorateur") }, "Dans l'Explorateur"),
        h("button", { class: "ghost small", title: "Fermer l'aperçu", onclick: () => (preview.hidden = true) }, "×"),
      ),
      data.binary
        ? h("p", { class: "note" }, "Fichier binaire : pas d'aperçu.")
        : h("pre", { class: "preview-text" }, data.text || "(fichier vide)"),
      data.truncated ? h("p", { class: "row-meta" }, "Aperçu limité aux 64 premiers Ko.") : null,
    );
  }

  const element = h(
    "aside",
    { class: "explorer", "aria-label": "Explorateur de fichiers" },
    h(
      "div",
      { class: "explorer-head" },
      title,
      h("button", { class: "ghost small", title: "Actualiser", onclick: () => root && void fill(tree, root, 0) }, "↻"),
      h(
        "button",
        { class: "ghost small", title: "Ouvrir le dossier dans l'Explorateur Windows", onclick: () => root && void open(root) },
        "Explorateur",
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
