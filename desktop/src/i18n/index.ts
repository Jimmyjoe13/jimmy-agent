/**
 * Traduction de l'interface (français, anglais).
 *
 * `t("clé", { variable })` rend le texte dans la langue courante. La langue
 * vient du réglage `ui.language` : « fr », « en », ou « auto » (langue de
 * Windows, lue par le navigateur intégré). Le français est la référence
 * (`fr.ts`) ; l'anglais est typé sur lui, donc complet par construction.
 */
import { en } from "./en";
import { fr, type Key, type Messages } from "./fr";

export type Lang = "fr" | "en";
export type LangSetting = "auto" | Lang;
export type { Key };

const DICTIONARIES: Record<Lang, Messages> = { fr, en };

let current: Lang = detect("auto");

/** Langue effective d'un réglage : « auto » suit le système (français
 *  si la langue de Windows est le français, anglais sinon). */
export function detect(setting: string | null | undefined, system = navigator.language): Lang {
  if (setting === "fr" || setting === "en") return setting;
  return system.toLowerCase().startsWith("fr") ? "fr" : "en";
}

/** Applique un réglage de langue (au démarrage, puis à chaque changement). */
export function setLanguage(setting: string | null | undefined): Lang {
  current = detect(setting);
  document.documentElement.lang = current;
  return current;
}

export function lang(): Lang {
  return current;
}

/** Texte traduit ; `{nom}` est remplacé par `vars.nom`. */
export function t(key: Key, vars?: Record<string, string | number>): string {
  const text = DICTIONARIES[current][key] ?? fr[key] ?? key;
  if (!vars) return text;
  return text.replace(/\{(\w+)\}/g, (whole, name: string) => (name in vars ? String(vars[name]) : whole));
}

/** Locale des dates et nombres (`Intl`). */
export function locale(): string {
  return current === "fr" ? "fr-FR" : "en-US";
}
