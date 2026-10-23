export type WorkDataInvalidationInput = {
  sourceWindowLabel?: string;
  changeSeq?: number;
  workspaceEpoch?: number;
  operationId?: string;
  coveredDomains?: string[];
  domains: string[];
};

export type WorkDataInvalidationContext = {
  currentWindowLabel: string;
  workspaceEpoch?: number;
  appliedChangeSeq?: number;
  knownOperationIds?: ReadonlySet<string>;
};

export type WorkDataInvalidationDecision = {
  action: "ignore" | "refresh" | "reset";
  domains: string[];
  changeSeq: number;
};

export function decideWorkDataInvalidation(
  input: WorkDataInvalidationInput,
  context: WorkDataInvalidationContext,
): WorkDataInvalidationDecision {
  const changeSeq = Math.max(0, input.changeSeq ?? 0);
  const domains = [...new Set(input.domains)];
  if (input.workspaceEpoch && context.workspaceEpoch && input.workspaceEpoch !== context.workspaceEpoch) {
    return { action: "reset", domains, changeSeq };
  }
  if (changeSeq > 0 && changeSeq <= (context.appliedChangeSeq ?? 0)) {
    return { action: "ignore", domains: [], changeSeq };
  }
  if (input.sourceWindowLabel && input.sourceWindowLabel === context.currentWindowLabel) {
    return { action: "ignore", domains: [], changeSeq };
  }
  if (input.operationId && context.knownOperationIds?.has(input.operationId)) {
    const covered = new Set(input.coveredDomains ?? []);
    const uncovered = domains.filter((domain) => !covered.has(domain));
    return { action: uncovered.length ? "refresh" : "ignore", domains: uncovered, changeSeq };
  }
  return { action: domains.length ? "refresh" : "ignore", domains, changeSeq };
}
