import axe from "axe-core";

export type AxeFinding = { id: string; impact: string | null | undefined; targets: string[] };

/**
 * Runs axe-core on a rendered container and returns the violations of impact serious or
 * critical. color-contrast is off: jsdom has no layout or canvas, so axe cannot compute it;
 * the token contrast contract tests cover contrast instead.
 */
export async function seriousAxeViolations(container: Element): Promise<AxeFinding[]> {
  const results = await axe.run(container, {
    rules: { "color-contrast": { enabled: false } },
    resultTypes: ["violations"],
  });
  return results.violations
    .filter((violation) => violation.impact === "serious" || violation.impact === "critical")
    .map((violation) => ({
      id: violation.id,
      impact: violation.impact,
      targets: violation.nodes.map((node) => node.target.join(" ")),
    }));
}
