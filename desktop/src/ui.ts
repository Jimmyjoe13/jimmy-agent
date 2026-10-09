/**
 * Micro-outil de rendu et état.
 *
 * Choix : pas de framework. L'interface de la V1 tient en quatre vues et un
 * panneau ; une dépendance de plus n'apporterait rien et alourdirait le
 * chargement de la fenêtre. On garde trois choses simples : un constructeur
 * d'éléments, un magasin réactif minimal, et un rendu par fonction.
 */

type Attrs = Record<string, string | number | boolean | EventListener | undefined | null>;
type Child = Node | string | number | null | undefined | false;

/** Crée un élément. `on*` devient un écouteur, `class` devient `className`. */
export function h<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  attrs: Attrs = {},
  ...children: Child[]
): HTMLElementTagNameMap[K] {
  const element = document.createElement(tag);
  for (const [key, value] of Object.entries(attrs)) {
    if (value === undefined || value === null || value === false) continue;
    if (key.startsWith("on") && typeof value === "function") {
      element.addEventListener(key.slice(2).toLowerCase(), value as EventListener);
    } else if (key === "class") {
      element.className = String(value);
    } else if (key === "html") {
      element.innerHTML = String(value);
    } else if (value === true) {
      element.setAttribute(key, "");
    } else {
      element.setAttribute(key, String(value));
    }
  }
  for (const child of children.flat()) {
    if (child === null || child === undefined || child === false) continue;
    element.append(typeof child === "object" ? child : document.createTextNode(String(child)));
  }
  return element;
}

export function clear(node: HTMLElement) {
  while (node.firstChild) node.removeChild(node.firstChild);
}

/** Remplace le contenu d'un conteneur. */
export function mount(container: HTMLElement, ...children: Child[]) {
  clear(container);
  for (const child of children.flat()) {
    if (child === null || child === undefined || child === false) continue;
    container.append(typeof child === "object" ? child : document.createTextNode(String(child)));
  }
}

/**
 * Icônes au trait (24×24, `currentColor`), reprises de la maquette Claude
 * Design. Des chaînes constantes : `html` ne reçoit jamais de donnée externe.
 */
const ICONS = {
  chat: '<path d="M21 12a8 8 0 0 1-11.6 7.1L4 20l1-4.6A8 8 0 1 1 21 12z"/>',
  voice: '<path d="M4 10v4M8 7v10M12 4v16M16 8v8M20 11v2"/>',
  history: '<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/>',
  memory: '<path d="M12 3l9 5-9 5-9-5 9-5z"/><path d="M3 13l9 5 9-5"/>',
  skills: '<path d="M12 3l2.5 6.5L21 12l-6.5 2.5L12 21l-2.5-6.5L3 12l6.5-2.5z"/>',
  skin:
    '<path d="M12 3a9 9 0 1 0 0 18c1.5 0 2-1 1.5-2s0-2 1.5-2h2a3 3 0 0 0 3-3A9 9 0 0 0 12 3z"/>' +
    '<circle cx="8" cy="11" r=".6"/><circle cx="12" cy="7.5" r=".6"/><circle cx="16" cy="11" r=".6"/>',
  settings: '<path d="M4 7h9M17 7h3M4 17h3M11 17h9"/><circle cx="15" cy="7" r="2"/><circle cx="9" cy="17" r="2"/>',
  diagnostic: '<path d="M3 12h4l2-6 4 12 2-6h6"/>',
  refresh: '<path d="M20 11a8 8 0 0 0-14.5-4M4 4v4h4"/><path d="M4 13a8 8 0 0 0 14.5 4M20 20v-4h-4"/>',
  folder: '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>',
  caret: '<path d="M6 9l6 6 6-6"/>',
  at: '<circle cx="12" cy="12" r="4"/><path d="M16 8v5a3 3 0 0 0 6 0v-1a10 10 0 1 0-4 8"/>',
  mic: '<rect x="9" y="3" width="6" height="12" rx="3"/><path d="M5 11a7 7 0 0 0 14 0M12 18v3"/>',
  send: '<path d="M12 19V5M5 12l7-7 7 7"/>',
} as const;

export type IconName = keyof typeof ICONS;

/** Icône SVG dans un `<span class="icon">` ; la taille suit `size` (px). */
export function icon(name: IconName, size = 18, strokeWidth = 1.7): HTMLSpanElement {
  return h("span", {
    class: "icon",
    "aria-hidden": "true",
    html:
      `<svg width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" ` +
      `stroke-width="${strokeWidth}" stroke-linecap="round" stroke-linejoin="round">${ICONS[name]}</svg>`,
  });
}

type Listener<T> = (state: T) => void;

/** Magasin réactif : trois lignes, et pas une de plus. */
export class Store<T> {
  private listeners = new Set<Listener<T>>();

  constructor(private state: T) {}

  get(): T {
    return this.state;
  }

  set(patch: Partial<T>) {
    this.state = { ...this.state, ...patch };
    for (const listener of this.listeners) listener(this.state);
  }

  subscribe(listener: Listener<T>) {
    this.listeners.add(listener);
    listener(this.state);
    return () => this.listeners.delete(listener);
  }
}

/** Affiche une erreur sans casser l'interface. */
export function toast(message: string, kind: "info" | "error" = "info") {
  let host = document.getElementById("toasts");
  if (!host) {
    host = h("div", { id: "toasts", class: "toasts" });
    document.body.append(host);
  }
  const node = h("div", { class: `toast ${kind}` }, message);
  host.append(node);
  setTimeout(() => {
    node.classList.add("leaving");
    setTimeout(() => node.remove(), 300);
  }, kind === "error" ? 7000 : 3500);
}

/** Exécute une action asynchrone en affichant ses erreurs. */
export async function guard<T>(action: () => Promise<T>, label = "action"): Promise<T | undefined> {
  try {
    return await action();
  } catch (error) {
    toast(`${label} : ${String(error)}`, "error");
    return undefined;
  }
}

/**
 * Comme `guard`, mais ne renvoie que la réussite.
 *
 * À utiliser dès que l'action retourne `void` : `guard` renvoie alors
 * `undefined` **dans les deux cas**, impossible à distinguer. C'est ce qui
 * faisait échouer le bouton d'enregistrement et l'activation de l'écoute —
 * l'action avait réussi, mais le code lisait l'échec.
 */
export async function attempt(
  action: () => Promise<unknown>,
  label = "action",
): Promise<boolean> {
  try {
    await action();
    return true;
  } catch (error) {
    toast(`${label} : ${String(error)}`, "error");
    return false;
  }
}

/** Libellés français partagés par les vues. */
export const QUALITY_LABEL: Record<string, string> = { low: "Basse", medium: "Moyenne", high: "Haute" };
export const MEMORY_KIND_LABEL: Record<string, string> = {
  semantic: "fait",
  procedural: "règle",
  episodic: "épisode",
};
export const STATE_LABEL: Record<string, string> = {
  idle: "prêt",
  listening: "j'écoute",
  thinking: "je réfléchis",
  speaking: "je parle",
  executing: "j'agis",
  success: "c'est fait",
  error: "il y a un souci",
  waiting: "j'attends",
};

/** Étapes de l'écoute, en une expression courte (pastille de la barre du haut). */
export const PHASE_LABEL: Record<string, string> = {
  idle: "prêt",
  capturing: "je t'entends",
  transcribing: "je transcris",
  your_turn: "à toi",
  thinking: "je réfléchis",
  speaking: "je parle",
};

/** « jimmy » → « Jimmy » (affichage du mot d'activation). */
export function capitalize(word: string): string {
  return word ? word[0].toUpperCase() + word.slice(1) : word;
}

export function formatTime(iso: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return "";
  return date.toLocaleString("fr-FR", {
    day: "2-digit",
    month: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}