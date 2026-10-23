export const TIMER_CALIBRATION_MIN_INTERVAL_MS = 30_000;
export const TIMER_CALIBRATION_FALLBACK_MS = 60_000;

export type TimerCalibrationState = {
  lastCalibratedAt: number;
  running: boolean;
  again: boolean;
};

export function shouldCalibrateTimer(
  now: number,
  lastCalibratedAt: number,
  wakeGapMs: number,
  realtimeHealthy = true,
) {
  return wakeGapMs > 15_000
    || now - lastCalibratedAt >= (realtimeHealthy ? TIMER_CALIBRATION_FALLBACK_MS : TIMER_CALIBRATION_MIN_INTERVAL_MS);
}

export class TimerCalibrationSingleFlight {
  private inFlight?: Promise<void>;
  private again = false;

  async run(calibrate: () => Promise<void>) {
    if (this.inFlight) {
      this.again = true;
      return await this.inFlight;
    }
    this.inFlight = (async () => {
      do {
        this.again = false;
        await calibrate();
      } while (this.again);
    })().finally(() => {
      this.inFlight = undefined;
    });
    return await this.inFlight;
  }
}
