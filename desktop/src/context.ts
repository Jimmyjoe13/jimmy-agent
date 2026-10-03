/** Contexte applicatif : l'état partagé que les vues reçoivent. */
import type { AgentEvent, Status, VoiceStatus } from "./api";

export type Route = "chat" | "history" | "voice" | "memory" | "skills" | "skin" | "settings" | "diagnostic";

/** Désabonnement. Appelé automatiquement quand la vue est quittée. */
export type Unsubscribe = () => void;

export interface AppContext {
  status: Status;
  /** État réel de l'écoute (null tant qu'il n'a pas été lu). */
  voice: VoiceStatus | null;
  /** Session ouverte dans le chat (reprise depuis l'historique). */
  lastSessionId: string | null;
  navigate: (route: Route) => void;
  /** Relit l'état (statut général + écoute) et met à jour la barre latérale. */
  refreshStatus: () => Promise<void>;
  /**
   * Événements de l'agent (état, outils, réponse, transcription…).
   *
   * Les abonnements pris pendant la construction d'une vue sont libérés
   * quand on la quitte : avant, chaque passage par le chat ajoutait un
   * écouteur de plus, qui continuait d'écrire dans une vue détachée.
   */
  onEvent: (handler: (event: AgentEvent) => void) => Unsubscribe;
  /** Action exécutée quand la vue est quittée (minuteries, micro…). */
  onCleanup: (cleanup: () => void) => void;
}
