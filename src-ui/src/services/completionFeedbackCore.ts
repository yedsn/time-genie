export type CompletionFeedbackMode = "none" | "normal" | "all-clear";

export type CompletionFeedbackDecisionInput = {
  completedCount: number;
  openBefore?: number;
  openAfter?: number;
  enabled: boolean;
};

export function decideCompletionFeedbackMode(input: CompletionFeedbackDecisionInput): CompletionFeedbackMode {
  if (!input.enabled || input.completedCount <= 0) return "none";
  if (typeof input.openBefore === "number" && input.openBefore > 0 && input.openAfter === 0) return "all-clear";
  return "normal";
}

export function completionFeedbackMessage(mode: CompletionFeedbackMode, completedCount: number) {
  if (mode === "all-clear") return "当前视图事项已全部完成";
  return completedCount > 1 ? `漂亮！一次完成 ${completedCount} 项` : "完成一项，继续保持";
}

export function remainingOpenTaskCount(openBefore: number | undefined, completedCount: number) {
  if (typeof openBefore !== "number") return undefined;
  return Math.max(0, openBefore - Math.max(0, completedCount));
}

export class CompletionFeedbackGate {
  private lastNormalAt = Number.NEGATIVE_INFINITY;

  constructor(private readonly normalCooldownMs = 220) {}

  shouldLaunch(mode: CompletionFeedbackMode, now: number) {
    if (mode === "none") return false;
    if (mode === "all-clear") return true;
    if (now - this.lastNormalAt < this.normalCooldownMs) return false;
    this.lastNormalAt = now;
    return true;
  }
}

export function runCompletionEffect(effect: () => void) {
  try {
    effect();
    return true;
  } catch {
    return false;
  }
}
