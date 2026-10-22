/** Vues Mémoire, Skills, Skin et Diagnostic. */
import { api, type DoctorReport, type McpServerStatus, type Memory, type Skill } from "../api";
import { attempt, formatTime, guard, h, memoryKindLabel, mount, qualityLabel, stateLabel, toast } from "../ui";
import type { AppContext } from "../context";
import { t, type Key } from "../i18n";
import { card, toggle } from "./settings";

/** Mémoire : ce que Jimy a retenu, et le droit de l'oublier. */
export function memoryView(ctx: AppContext): HTMLElement {
  const container = h("section", { class: "view" });
  const list = h("div", { class: "list" });

  async function reload() {
    const memories = await guard(() => api.memoryList(), t("guard.memory"));
    if (!memories) return;
    if (memories.length === 0) {
      mount(
        list,
        h(
          "div",
          { class: "empty" },
          h("p", {}, t("memory.empty")),
          h("p", { class: "hint" }, t("memory.emptyHint")),
        ),
      );
      return;
    }
    mount(list);
    for (const memory of memories as Memory[]) {
      list.append(
        h(
          "div",
          { class: "list-row" },
          h(
            "div",
            {},
            h("span", { class: `tag kind-${memory.kind}` }, memoryKindLabel(memory.kind)),
            h("p", {}, memory.content),
            h(
              "div",
              { class: "row-meta" },
              t("memory.meta", { date: formatTime(memory.createdAt), importance: memory.importance.toFixed(2), uses: memory.useCount }),
            ),
          ),
          h(
            "div",
            { class: "row-actions" },
            h(
              "button",
              {
                class: "danger",
                onclick: async () => {
                  if (!window.confirm(t("memory.forgetConfirm"))) return;
                  if (!(await attempt(() => api.memoryForget(memory.id), "oubli"))) return;
                  await ctx.refreshStatus();
                  await reload();
                },
              },
              t("memory.forget"),
            ),
          ),
        ),
      );
    }
  }

  mount(
    container,
    h(
      "header",
      { class: "view-header" },
      h("button", { class: "ghost", onclick: () => void reload() }, t("common.refresh")),
    ),
    card(
      t("memory.title"),
      h(
        "p",
        { class: "note" },
        t("memory.stored"),
        ctx.status.memory.semantic_model
          ? t("memory.semantic")
          : t("memory.keywords"),
      ),
      h(
        "div",
        { class: "stat-row" },
        stat(String(ctx.status.memory.count), t("memory.stat.memories")),
        stat(String(ctx.status.tools.length), t("memory.stat.tools")),
        stat(String(ctx.status.skills), t("memory.stat.skills")),
      ),
      list,
    ),
  );
  void reload();
  return container;
}

function stat(value: string, label: string): HTMLElement {
  return h("div", { class: "stat" }, h("strong", {}, value), h("span", {}, label));
}

/** Libellé et classe de l'état d'un serveur MCP. */
const MCP_STATE: Record<McpServerStatus["state"], { label: Key; cls: string }> = {
  connected: { label: "tools.mcp.connected", cls: "ok" },
  stopped: { label: "tools.mcp.stopped", cls: "" },
  busy: { label: "tools.mcp.busy", cls: "warn" },
  disabled: { label: "tools.mcp.disabled", cls: "" },
};

/**
 * Skills : deux sous-onglets. « Skills » : les procédures que Jimy sait
 * appliquer, et qu'il peut créer. « Serveurs MCP » : les serveurs connectés
 * (ajoutés en config ou par Jimy via `mcp_add_server`) et leurs outils.
 */
export function skillsView(): HTMLElement {
  const container = h("section", { class: "view" });
  const skillsPane = h("div", {});
  const mcpPane = h("div", {});
  let tab: "skills" | "mcp" = "skills";
  const tabButtons = {
    skills: h("button", { class: "subtab", role: "tab", onclick: () => select("skills") }, t("tools.tab.skills")),
    mcp: h("button", { class: "subtab", role: "tab", onclick: () => select("mcp") }, t("tools.tab.mcp")),
  };

  function select(next: "skills" | "mcp") {
    tab = next;
    for (const [key, button] of Object.entries(tabButtons)) {
      const active = key === tab;
      button.classList.toggle("active", active);
      button.setAttribute("aria-selected", String(active));
    }
    skillsPane.hidden = tab !== "skills";
    mcpPane.hidden = tab !== "mcp";
    void (tab === "skills" ? reload() : reloadMcp());
  }

  const list = h("div", { class: "list" });
  const mcpList = h("div", { class: "list" });

  /** Serveurs MCP : état réel, commande (secrets masqués) et outils. */
  async function reloadMcp() {
    const servers = await guard(() => api.mcpServers(), t("guard.mcp"));
    if (!servers) return;
    if (servers.length === 0) {
      mount(
        mcpList,
        h(
          "div",
          { class: "empty" },
          h("p", {}, t("tools.mcp.none")),
          h("p", { class: "hint" }, t("tools.mcp.noneHint")),
        ),
      );
      return;
    }
    mount(mcpList);
    for (const server of servers) {
      const known = MCP_STATE[server.state];
      const state = known ? { label: t(known.label), cls: known.cls } : { label: server.state, cls: "" };
      const count = server.tools.length;
      mcpList.append(
        h(
          "details",
          { class: "list-row skill mcp-server" },
          h(
            "summary",
            {},
            h("strong", {}, server.name),
            " ",
            h("span", { class: `badge ${state.cls}` }, state.label),
            " ",
            h("span", { class: "row-meta" }, t("tools.mcp.toolCount", { count, transport: server.transport })),
          ),
          h("p", { class: "row-meta" }, t("tools.mcp.command")),
          h("pre", {}, server.launch || "—"),
          server.envKeys.length
            ? h("p", { class: "row-meta" }, t("tools.mcp.env", { keys: server.envKeys.join(", ") }))
            : null,
          count
            ? h(
                "ul",
                { class: "mcp-tools" },
                ...server.tools.map((tool) =>
                  h("li", {}, h("code", {}, tool.name), tool.description ? ` — ${tool.description}` : ""),
                ),
              )
            : h("p", { class: "note" }, t("tools.mcp.noTools")),
        ),
      );
    }
  }

  async function reload() {
    const skills = await guard(() => api.skillsList(), "skills");
    if (!skills) return;
    if (skills.length === 0) {
      mount(
        list,
        h(
          "div",
          { class: "empty" },
          h("p", {}, t("tools.skills.none")),
          h(
            "p",
            { class: "hint" },
            t("tools.skills.noneHint"),
          ),
        ),
      );
      return;
    }
    mount(list);
    for (const skill of skills as Skill[]) {
      list.append(
        h(
          "details",
          { class: "list-row skill" },
          h("summary", {}, h("strong", {}, skill.name)),
          h("p", { class: "note" }, skill.description),
          h("pre", {}, skill.body),
        ),
      );
    }
  }

  mount(
    skillsPane,
    card(
      t("tools.skills.folder"),
      h(
        "p",
        { class: "note" },
        t("tools.skills.folderNote"),
      ),
      list,
    ),
  );
  mount(
    mcpPane,
    card(
      t("tools.mcp.connectedTitle"),
      h(
        "p",
        { class: "note" },
        t("tools.mcp.connectedNote"),
      ),
      mcpList,
    ),
  );
  mount(
    container,
    h(
      "header",
      { class: "view-header" },
      h("div", { class: "subtabs", role: "tablist" }, tabButtons.skills, tabButtons.mcp),
      h("button", { class: "ghost", onclick: () => void (tab === "skills" ? reload() : reloadMcp()) }, t("common.refresh")),
    ),
    skillsPane,
    mcpPane,
  );
  select("skills");
  return container;
}

/** Skin : choix de l'apparence de Jimy, appliqué à chaud par Godot. */
export function skinView(ctx: AppContext): HTMLElement {
  // Bouton d'un skin applicable (de base, ou premium débloqué).
  const skinButton = (skin: { id: string; label: string }) =>
    h(
      "button",
      {
        class: ctx.status.avatar.skin === skin.id ? "primary" : "ghost",
        onclick: async () => {
          // `attempt` et non `guard` : la commande ne renvoie rien (piège 13).
          if (!(await attempt(() => api.avatarSkin(skin.id), "skin"))) return;
          await ctx.refreshStatus();
          // Re-rendu : le bouton du skin actif doit changer d'apparence.
          ctx.navigate("skin");
          toast(t("skin.applied", { label: skin.label }));
        },
      },
      skin.label,
    );
  const skinButtons = h("div", { class: "row" }, ...ctx.status.avatar.skins.map(skinButton));

  // Skins premium : applicables s'ils sont débloqués, sinon à acheter. La
  // clé reçue après l'achat (page Merci du site) se colle ici.
  const premium = ctx.status.avatar.premium;
  const premiumButtons = h(
    "div",
    { class: "row" },
    ...premium.map((skin) =>
      skin.unlocked
        ? skinButton(skin)
        : h("button", { class: "ghost premium-locked", disabled: true, title: t("skin.locked") }, `🔒 ${skin.label}`),
    ),
  );
  const keyInput = h("input", { class: "field", placeholder: "JIMY-…", spellcheck: "false" }) as HTMLInputElement;
  const activateButton = h(
    "button",
    {
      class: "primary",
      onclick: async () => {
        const key = keyInput.value.trim();
        if (!key) return;
        // Les fichiers du skin sont téléchargés à l'activation (quelques Mo).
        toast(t("skin.activating"));
        const unlocked = await guard(() => api.licenceActivate(key), "licence");
        if (unlocked === undefined) return;
        await ctx.refreshStatus();
        ctx.navigate("skin");
        const label = premium.find((s) => s.id === unlocked)?.label;
        toast(unlocked === "*" ? t("skin.allUnlocked") : t("skin.unlocked", { label: label ?? unlocked }));
      },
    },
    t("skin.activate"),
  );
  const allUnlocked = premium.every((s) => s.unlocked);

  const qualityButtons = h(
    "div",
    { class: "row" },
    ...["low", "medium", "high"].map((level) =>
      h(
        "button",
        {
          class: ctx.status.avatar.quality === level ? "primary" : "ghost",
          onclick: async () => {
            if (!(await attempt(() => api.avatarQuality(level), t("guard.quality")))) return;
            await ctx.refreshStatus();
            ctx.navigate("skin");
            toast(t("skin.qualityApplied", { label: qualityLabel(level) }));
          },
        },
        qualityLabel(level),
      ),
    ),
  );

  const bubbleInput = h("input", { class: "field", value: t("skin.bubbleDefault") }) as HTMLInputElement;
  const dodgeToggle = h("input", { type: "checkbox" }) as HTMLInputElement;
  dodgeToggle.checked = ctx.status.avatar.dodge;
  dodgeToggle.addEventListener("change", async () => {
    const enabled = dodgeToggle.checked;
    if (!(await attempt(() => api.avatarDodge(enabled), "esquive"))) {
      dodgeToggle.checked = !enabled;
      return;
    }
    await ctx.refreshStatus();
    toast(enabled ? t("skin.dodgeOn") : t("skin.dodgeOff"));
  });

  return h(
    "section",
    { class: "view" },
    ctx.status.avatar.running
      ? null
      : h(
          "div",
          { class: "warn-note" },
          t("skin.avatarStopped"),
        ),
    card(
      t("skin.active"),
      skinButtons,
      h(
        "p",
        { class: "note" },
        t("skin.activeNote"),
      ),
    ),
    card(
      t("skin.premium"),
      premiumButtons,
      allUnlocked
        ? h(
            "p",
            { class: "note" },
            t("skin.premiumAll", { list: premium.map((s) => `${s.label} → « ${s.wake_word} »`).join(", ") }),
          )
        : h(
            "div",
            {},
            h(
              "p",
              { class: "note" },
              t("skin.licenseHint"),
            ),
            h("div", { class: "row" }, keyInput, activateButton),
            h(
              "div",
              { class: "row" },
              h(
                "button",
                { class: "ghost", onclick: () => attempt(() => api.shopOpen(), "boutique") },
                t("skin.shop"),
              ),
            ),
          ),
    ),
    card(t("skin.quality"), h("p", { class: "note" }, t("skin.qualityNote")), qualityButtons),
    card(
      t("skin.behavior"),
      h(
        "p",
        { class: "note" },
        t("skin.behaviorNote"),
      ),
      toggle(t("skin.dodge"), dodgeToggle),
    ),
    card(
      t("skin.states"),
      h(
        "div",
        { class: "state-grid" },
        ...["idle", "listening", "thinking", "speaking", "executing", "success", "error", "waiting"].map(
          (state) =>
            h(
              "button",
              {
                class: "state-chip",
                onclick: async () => {
                  await attempt(() => api.avatarState(state as never, ""), t("guard.state"));
                },
              },
              stateLabel(state),
            ),
        ),
      ),
      h(
        "p",
        { class: "note" },
        t("skin.statesNote"),
      ),
    ),
    card(
      t("skin.bubbleTest"),
      h(
        "div",
        { class: "row" },
        bubbleInput,
        h(
          "button",
          { class: "ghost", onclick: () => void attempt(() => api.avatarSay(bubbleInput.value), "bulle") },
          t("skin.bubbleShow"),
        ),
      ),
    ),
  );
}

/** Diagnostic : ce qui est prêt, ce qui manque, où sont les fichiers. */
export function diagnosticView(ctx: AppContext): HTMLElement {
  const container = h("section", { class: "view" });
  const report = h("div", { class: "list" });
  const paths = h("pre", { class: "paths" }, "…");

  async function run() {
    const result = await guard(() => api.doctor(), "diagnostic");
    mount(report);
    if (!result) {
      report.append(h("div", { class: "empty" }, h("p", {}, t("diag.impossible"))));
      return;
    }
    const typed = result as DoctorReport;
    report.append(
      h(
        "p",
        { class: typed.ok ? "ok-note" : "warn-note" },
        t("diag.summary", { passed: typed.passed, total: typed.total }),
      ),
    );
    for (const check of typed.checks) {
      report.append(
        h(
          "div",
          { class: `list-row check ${check.ok ? "ok" : "ko"}` },
          h("strong", {}, check.ok ? "✓" : "✗", " ", check.name),
          h("span", { class: "note" }, check.ok ? "" : check.hint),
        ),
      );
    }
  }

  async function showPaths() {
    const info = await guard(() => api.pathsInfo(), "chemins");
    if (!info) return;
    paths.textContent = JSON.stringify(info, null, 2);
  }

  mount(
    container,
    h(
      "header",
      { class: "view-header" },
      h(
        "div",
        { class: "row" },
        h(
          "button",
          {
            class: "ghost",
            onclick: async () => {
              const result = await guard(() => api.reloadSecrets(), "secrets");
              if (!result) return;
              toast(
                result.missing.length
                  ? t("diag.secretsMissing", { missing: result.missing.join(", ") })
                  : t("diag.secretsReloaded"),
                result.missing.length ? "error" : "info",
              );
              await ctx.refreshStatus();
              await run();
            },
          },
          t("diag.reloadEnv"),
        ),
        h("button", { class: "primary", onclick: () => void run() }, t("diag.rerun")),
      ),
    ),
    card(t("diag.checks"), report),
    card(t("diag.files"), h("p", { class: "note" }, t("diag.log")), paths),
  );
  void run();
  void showPaths();
  return container;
}