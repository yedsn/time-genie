export function normalizeAllocationMinutes(value: unknown) {
  const minutes = Number(value);
  return Number.isFinite(minutes) ? Math.max(0, Math.round(minutes)) : 0;
}

export function allocationMaximum(minutes: unknown[], index: number, totalMinutes: number) {
  const total = normalizeAllocationMinutes(totalMinutes);
  const pairedIndex = index > 0 ? index - 1 : minutes.length > 1 ? 1 : -1;
  const fixedMinutes = minutes.reduce<number>((sum, value, currentIndex) => {
    if (currentIndex === index || currentIndex === pairedIndex) return sum;
    return sum + normalizeAllocationMinutes(value);
  }, 0);
  return Math.max(0, total - fixedMinutes);
}

export function rebalanceAllocationMinutes(
  minutes: unknown[],
  index: number,
  requestedMinutes: unknown,
  totalMinutes: number,
) {
  const next = minutes.map(normalizeAllocationMinutes);
  if (index < 0 || index >= next.length) return next;

  const maximum = allocationMaximum(next, index, totalMinutes);
  const current = Math.min(normalizeAllocationMinutes(requestedMinutes), maximum);
  next[index] = current;
  const pairedIndex = index > 0 ? index - 1 : next.length > 1 ? 1 : -1;
  if (pairedIndex >= 0) next[pairedIndex] = maximum - current;
  return next;
}

export function partialAllocationMaximum(minutes: unknown[], index: number, totalMinutes: number) {
  const total = normalizeAllocationMinutes(totalMinutes);
  const fixedMinutes = minutes.reduce<number>((sum, value, currentIndex) => {
    if (currentIndex === index) return sum;
    return sum + normalizeAllocationMinutes(value);
  }, 0);
  return Math.max(0, total - fixedMinutes);
}

export function clampPartialAllocationMinutes(
  minutes: unknown[],
  index: number,
  requestedMinutes: unknown,
  totalMinutes: number,
) {
  const next = minutes.map(normalizeAllocationMinutes);
  if (index < 0 || index >= next.length) return next;

  const maximum = partialAllocationMaximum(next, index, totalMinutes);
  next[index] = Math.min(normalizeAllocationMinutes(requestedMinutes), maximum);
  return next;
}
