export type TaskSubmissionContext = {
  taskId: string;
  rebindTaskId: (taskId: string) => void;
};

type TaskSubmissionOperation<Snapshot, Result> = (snapshot: Snapshot, context: TaskSubmissionContext) => Promise<Result>;

type PendingSubmission<Snapshot, Result> = {
  snapshot: Snapshot;
  operation: TaskSubmissionOperation<Snapshot, Result>;
};

type SubmissionSlot<Snapshot, Result> = {
  key: string;
  taskId: string;
  aliases: Set<string>;
  pending?: PendingSubmission<Snapshot, Result>;
  running: boolean;
  timer?: ReturnType<typeof setTimeout>;
  promise: Promise<Result>;
  resolve: (result: Result) => void;
  reject: (error: unknown) => void;
  lastResult?: Result;
};

export class TaskSubmissionCoordinator<Snapshot, Result> {
  private readonly slots = new Map<string, SubmissionSlot<Snapshot, Result>>();
  private readonly taskKeys = new Map<string, string>();

  constructor(private readonly delayMs = 250) {}

  schedule(taskId: string, snapshot: Snapshot, operation: TaskSubmissionOperation<Snapshot, Result>) {
    const key = this.taskKeys.get(taskId) ?? taskId;
    let slot = this.slots.get(key);
    if (!slot) {
      slot = this.createSlot(key, taskId);
      this.slots.set(key, slot);
    }
    this.bindTaskId(slot, taskId);
    slot.pending = { snapshot, operation };
    if (!slot.running) this.arm(slot);
    return slot.promise;
  }

  async flush(taskId: string) {
    const slot = this.slotForTask(taskId);
    if (!slot) return undefined;
    if (slot.timer) clearTimeout(slot.timer);
    slot.timer = undefined;
    if (!slot.running) void this.run(slot);
    return await slot.promise;
  }

  hasPending(taskId: string) {
    return Boolean(this.slotForTask(taskId));
  }

  private createSlot(key: string, taskId: string) {
    let resolve!: (result: Result) => void;
    let reject!: (error: unknown) => void;
    const promise = new Promise<Result>((resolvePromise, rejectPromise) => {
      resolve = resolvePromise;
      reject = rejectPromise;
    });
    return { key, taskId, aliases: new Set<string>(), running: false, promise, resolve, reject } satisfies SubmissionSlot<Snapshot, Result>;
  }

  private slotForTask(taskId: string) {
    const key = this.taskKeys.get(taskId) ?? taskId;
    return this.slots.get(key);
  }

  private bindTaskId(slot: SubmissionSlot<Snapshot, Result>, taskId: string) {
    slot.taskId = taskId;
    slot.aliases.add(taskId);
    this.taskKeys.set(taskId, slot.key);
  }

  private arm(slot: SubmissionSlot<Snapshot, Result>) {
    if (slot.timer) clearTimeout(slot.timer);
    slot.timer = setTimeout(() => {
      slot.timer = undefined;
      void this.run(slot);
    }, this.delayMs);
  }

  private async run(slot: SubmissionSlot<Snapshot, Result>) {
    if (slot.running || this.slots.get(slot.key) !== slot) return;
    const submission = slot.pending;
    if (!submission) {
      this.finish(slot);
      return;
    }
    slot.pending = undefined;
    slot.running = true;
    try {
      const context: TaskSubmissionContext = {
        get taskId() { return slot.taskId; },
        rebindTaskId: (taskId) => this.bindTaskId(slot, taskId),
      };
      slot.lastResult = await submission.operation(submission.snapshot, context);
      slot.running = false;
      if (slot.pending) {
        void this.run(slot);
        return;
      }
      this.finish(slot);
    } catch (error) {
      slot.running = false;
      this.cleanup(slot);
      slot.reject(error);
    }
  }

  private finish(slot: SubmissionSlot<Snapshot, Result>) {
    if (slot.lastResult === undefined) return;
    const result = slot.lastResult;
    this.cleanup(slot);
    slot.resolve(result);
  }

  private cleanup(slot: SubmissionSlot<Snapshot, Result>) {
    if (slot.timer) clearTimeout(slot.timer);
    this.slots.delete(slot.key);
    for (const taskId of slot.aliases) {
      if (this.taskKeys.get(taskId) === slot.key) this.taskKeys.delete(taskId);
    }
  }
}
