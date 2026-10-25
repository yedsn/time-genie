export type HoverFrameScheduler = {
  request: (callback: () => void) => number;
  cancel: (handle: number) => void;
};

export function createHoverFrameCommitter(scheduler: HoverFrameScheduler) {
  let pending: (() => void) | undefined;
  let frame = 0;

  const cancel = () => {
    pending = undefined;
    if (!frame) return;
    scheduler.cancel(frame);
    frame = 0;
  };

  const schedule = (commit: () => void) => {
    pending = commit;
    if (frame) return;
    frame = scheduler.request(() => {
      frame = 0;
      const next = pending;
      pending = undefined;
      next?.();
    });
  };

  return { schedule, cancel };
}
