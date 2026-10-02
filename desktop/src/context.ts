/** Contexte applicatif : l'état partagé que les vues reçoivent. */
import type { Status } from "./api";

export type Route = "chat" | "history" | "voice" | "memory" | "skills" | "skin" | "settings" | "diagnostic";

export interface AppContext {
  status: Status;
  lastSessionId: string | null;
  navigate: (route: Route) => void;
  refreshStatus: () => Promise<void>;
  /** Notifie les vues de l'état de l'avatar. */
  onState: (handler: (state: string) => void) => void;
  emitState: (state: string) => void;
  /** Journal d'activité (outils, mémoire, Synaptiq). */
  onActivity: (handler: (entry: HTMLElement) => void) => void;
  emitActivity: (entry: HTMLElement) => void;
  /** Réponse finale de Jimmy à afficher dans le fil. */
  pushAssistant: (text: string) => void;
}