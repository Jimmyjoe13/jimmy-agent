---
name: jimy-seo
description: Bot SEO du site jimy-agent.me (agent IA Jimy). À utiliser pour les tâches planifiées jimy-seo-* et pour tout message de Jimmy commençant par « SEO » (SEO OK, SEO CORRIGE, SEO ABANDON, SEO FAIT, SEO ETAT, SEO RAPPORT, SEO PAUSE, SEO REPRISE, SEO ANNULE).
metadata:
  domain: seo
  owner: Jimmy
  source: docs/seo/hermes/SKILL.md du dépôt Jimmyjoe13/jimmy-agent (ne pas éditer la copie installée)
---

# Skill : jimy-seo

Ce skill n'est qu'un **aiguillage**. Toutes les règles vivent dans le dépôt, pour qu'une seule
version fasse foi :

```
/home/jimmy/jimy-seo/jimmy-agent/docs/seo/
```

## À chaque déclenchement

1. `cd /home/jimmy/jimy-seo/jimmy-agent && git fetch origin --prune && git checkout main && git pull --ff-only origin main`
2. Lire `docs/seo/AGENTS.md` (point d'entrée), puis les fichiers qu'il désigne.
3. Appliquer la procédure correspondant au déclencheur (tableau « Selon ce qui t'a lancé »
   de `AGENTS.md`). Un message de Jimmy commençant par `SEO` → procédure P2.

## Commande Telegram `/seo` (raccourci vers ce skill)

`/seo` est un alias de ce skill. Le texte qui suit `/seo` est une commande P2 **sans le mot
SEO** : `/seo ok S01` = `SEO OK S01`, `/seo corrige S01 raccourcis l'intro` =
`SEO CORRIGE S01 : raccourcis l'intro`, `/seo abandon S01`, `/seo pause`, etc.
`/seo` **seul** = `SEO ETAT` (message M8).
`/seo_rapport` et `/seo_derniere` sont des scripts lancés directement par Hermes, sans toi.

## Rappels qui ne doivent jamais être oubliés

- Rien n'est mis en ligne sans « SEO OK <ID> » de Jimmy.
- Aucun secret dans le dépôt : il est public.
- En cas d'échec ou de doute : procédure P7 (message d'incident), pas d'improvisation.
- Rien à signaler à la fin d'une tâche planifiée : répondre exactement `[SILENT]`.

## Mise à jour de ce skill

Modifier `docs/seo/hermes/SKILL.md` dans le dépôt, puis recopier :
`cp /home/jimmy/jimy-seo/jimmy-agent/docs/seo/hermes/SKILL.md ~/.hermes/skills/jimy-seo/SKILL.md`
