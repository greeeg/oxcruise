import type { Model } from "../data/model";
import type { Nav } from "../state/nav";
import { el } from "../ui/ui";

/** The violations, grouped by rule, with filters. */
export class ViolationsView {
  readonly root = el("div", { class: "violations" });

  constructor(
    host: HTMLElement,
    private readonly model: Model,
    private readonly nav: Nav,
  ) {
    host.append(this.root);
  }

  show(): void {
    const { model, nav } = this;
    const s = nav.state();
    const data = model.data;
    const sev = el("select", { class: "toolbar-select" }, [
      el("option", { text: "All severities", attrs: { value: "" } }),
      el("option", { text: "Errors", attrs: { value: "error" } }),
      el("option", { text: "Warnings", attrs: { value: "warn" } }),
    ]);
    sev.value = s.sev ?? "";
    const rule = el("select", { class: "toolbar-select" }, [
      el("option", { text: "All rules", attrs: { value: "" } }),
      ...data.rules.map((r) => el("option", { text: r, attrs: { value: r } })),
    ]);
    rule.value = s.rule ?? "";
    const change = () =>
      nav.go({ sev: sev.value || undefined, rule: rule.value || undefined }, true);
    sev.addEventListener("change", change);
    rule.addEventListener("change", change);

    const groups = new Map<number, number[]>();
    data.violations.forEach((v, i) => {
      if (s.sev && v.level !== s.sev) return;
      if (s.rule && data.rules[v.rule] !== s.rule) return;
      const g = groups.get(v.rule);
      if (g) g.push(i);
      else groups.set(v.rule, [i]);
    });
    const body = el("div", { class: "groups" });
    if (groups.size === 0) {
      body.append(
        el("p", {
          class: "muted",
          text:
            data.violations.length === 0 ? "No violations." : "No violation matches the filters.",
        }),
      );
    }
    for (const [r, list] of [...groups].sort((a, b) => b[1].length - a[1].length)) {
      body.append(
        el("section", {}, [
          el("h3", { text: `${data.rules[r]} (${list.length})` }),
          el(
            "ul",
            { class: "plain" },
            list.slice(0, 500).map((i) => this.item(i)),
          ),
          ...(list.length > 500
            ? [el("p", { class: "muted", text: `and ${list.length - 500} more` })]
            : []),
        ]),
      );
    }
    this.root.replaceChildren(el("div", { class: "toolbar" }, [sev, rule]), body);
  }

  private item(i: number): HTMLElement {
    const { model, nav } = this;
    const v = model.data.violations[i];
    const from = v.from !== undefined ? model.modPaths[v.from] : (v.file ?? "");
    const to = v.to !== undefined ? ` → ${model.modPaths[v.to]}` : "";
    const li = el("li", { class: v.from !== undefined ? "clickable" : "" }, [
      el("span", { class: `dot sev-${v.level}`, title: v.level }),
      el("div", {}, [
        el("div", { text: v.message }),
        el("div", {
          class: "muted small",
          text: `${from}${to}${v.cycle?.length ? ` · cycle of ${v.cycle.length}` : ""}`,
        }),
      ]),
    ]);
    if (v.from !== undefined) {
      li.addEventListener("click", () =>
        nav.focus(model.nodeOfMod(v.from!), { cycle: v.cycle?.length ? i : undefined }),
      );
    }
    return li;
  }
}
