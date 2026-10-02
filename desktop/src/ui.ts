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