/**
 * Barre de titre de la fenêtre (remplace la barre native de Windows,
 * `decorations: false` dans tauri.conf.json).
 *
 * Bande transparente posée au-dessus de l'interface : la barre latérale et
 * la zone principale montent jusqu'en haut, sans bande grise détachée.
 * - glisser n'importe où sur la bande déplace la fenêtre, double-clic =
 *   agrandir / restaurer (`data-tauri-drag-region`) ;
 * - fermer passe par `close()` : l'événement `CloseRequested` côté Rust
 *   cache la fenêtre (Jimmy reste vivant), comme l'ancien bouton natif.
 */
import { getCurrentWindow } from "@tauri-apps/api/window";
import { h, icon } from "./ui";

export function mountTitlebar() {
  const win = getCurrentWindow();

  const maximize = h(
    "button",
    { class: "win-btn", title: "Agrandir", "aria-label": "Agrandir", onclick: () => win.toggleMaximize() },
    icon("winMaximize", 14, 1.5),
  );

  // L'icône suit l'état réel (agrandie ou non), y compris après un
  // double-clic sur la bande ou un raccourci Windows (Win+↑).
  const syncMaximized = async () => {
    const maximized = await win.isMaximized();
    const label = maximized ? "Restaurer" : "Agrandir";
    maximize.title = label;
    maximize.setAttribute("aria-label", label);
    maximize.replaceChildren(icon(maximized ? "winRestore" : "winMaximize", 14, 1.5));
  };

  const bar = h(
    "div",
    { class: "titlebar", "data-tauri-drag-region": true },
    h(
      "div",
      { class: "win-controls" },
      h(
        "button",
        { class: "win-btn", title: "Réduire", "aria-label": "Réduire", onclick: () => win.minimize() },
        icon("winMinimize", 14, 1.5),
      ),
      maximize,
      h(
        "button",
        { class: "win-btn win-close", title: "Fermer", "aria-label": "Fermer", onclick: () => win.close() },
        icon("winClose", 14, 1.5),
      ),
    ),
  );

  document.body.prepend(bar);
  void syncMaximized();
  void win.onResized(() => void syncMaximized());
}
