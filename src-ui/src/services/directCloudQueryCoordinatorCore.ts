export type DirectCloudQueryMetric =
  | "started"
  | "reused"
  | "skipped_fresh"
  | "invalidated"
  | "followup"
  | "discarded_epoch";

export type DirectCloudQueryState = {
  loadedAt: number;
  appliedSequence: number;
  staleSequence: number;
  epoch: number;
  loading: boolean;
};

type InternalQueryState = DirectCloudQueryState & {
  generation: number;
  refreshAgain: boolean;
  inFlight?: Promise<unknown>;
};

export type DirectCloudQueryRunOptions = {
  force?: boolean;
  maxAgeMs?: number;
  sequence?: number;
  epoch?: number;
};

export class DirectCloudQueryCoordinator {
  private readonly states = new Map<string, InternalQueryState>();
  private generation = 1;

  constructor(
    private readonly now: () => number = () => Date.now(),
    private readonly onMetric?: (metric: DirectCloudQueryMetric, key: string) => void,
  ) {}

  snapshot(key: string): DirectCloudQueryState | undefined {
    const state = this.states.get(key);
    if (!state) return undefined;
    return {
      loadedAt: state.loadedAt,
      appliedSequence: state.appliedSequence,
      staleSequence: state.staleSequence,
      epoch: state.epoch,
      loading: Boolean(state.inFlight),
    };
  }

  markStale(key: string, sequence = 0, epoch?: number) {
    const state = this.ensureState(key, epoch);
    state.staleSequence = Math.max(state.staleSequence, sequence || state.appliedSequence + 1);
    if (state.inFlight) state.refreshAgain = true;
    this.onMetric?.("invalidated", key);
  }

  markApplied(key: string, sequence = 0, epoch?: number) {
    const state = this.ensureState(key, epoch);
    state.appliedSequence = Math.max(state.appliedSequence, sequence);
    state.staleSequence = Math.min(state.staleSequence, state.appliedSequence);
    state.loadedAt = this.now();
  }

  isStale(key: string) {
    const state = this.states.get(key);
    return !state || state.staleSequence > state.appliedSequence;
  }

  clear() {
    this.generation += 1;
    this.states.clear();
  }

  clearMatching(predicate: (key: string) => boolean) {
    this.generation += 1;
    for (const key of this.states.keys()) if (predicate(key)) this.states.delete(key);
  }

  markMatchingStale(prefix: string, sequence = 0, epoch?: number) {
    let matched = false;
    for (const key of this.states.keys()) {
      if (key === prefix || key.startsWith(prefix + ":")) {
        this.markStale(key, sequence, epoch);
        matched = true;
      }
    }
    if (!matched) this.markStale(prefix, sequence, epoch);
  }

  async run<T>(
    key: string,
    loader: (isCurrent: () => boolean) => Promise<T>,
    options: DirectCloudQueryRunOptions = {},
  ): Promise<T | undefined> {
    const state = this.ensureState(key, options.epoch);
    const maxAgeMs = Math.max(0, options.maxAgeMs ?? 0);
    const fresh = state.loadedAt > 0
      && state.staleSequence <= state.appliedSequence
      && this.now() - state.loadedAt <= maxAgeMs;
    if (!options.force && fresh) {
      this.onMetric?.("skipped_fresh", key);
      return undefined;
    }
    if (state.inFlight) {
      this.onMetric?.("reused", key);
      return await state.inFlight as T | undefined;
    }

    const generation = this.generation;
    const run = async () => {
      let result: T | undefined;
      let attempts = 0;
      do {
        state.refreshAgain = false;
        attempts += 1;
        this.onMetric?.(attempts === 1 ? "started" : "followup", key);
        const isCurrent = () => generation === this.generation && this.states.get(key) === state;
        result = await loader(isCurrent);
        if (generation !== this.generation || this.states.get(key) !== state) {
          this.onMetric?.("discarded_epoch", key);
          return undefined;
        }
        state.loadedAt = this.now();
        state.appliedSequence = Math.max(
          state.appliedSequence,
          options.sequence ?? 0,
          state.staleSequence,
        );
      } while (state.refreshAgain && attempts < 2);
      return result;
    };
    state.inFlight = run().finally(() => {
      if (this.states.get(key) === state) state.inFlight = undefined;
    });
    return await state.inFlight as T | undefined;
  }

  private ensureState(key: string, epoch?: number) {
    const requestedEpoch = epoch ?? 0;
    const existing = this.states.get(key);
    if (existing && (!requestedEpoch || !existing.epoch || existing.epoch === requestedEpoch)) return existing;
    const state: InternalQueryState = {
      loadedAt: 0,
      appliedSequence: 0,
      staleSequence: 0,
      epoch: requestedEpoch,
      loading: false,
      generation: this.generation,
      refreshAgain: false,
    };
    this.states.set(key, state);
    return state;
  }
}

export function directCloudQueryKey(domain: string, ...parts: Array<string | number | undefined>) {
  return [domain, ...parts.map((part) => part ?? "")].join(":");
}
