export type VersionedEntity = {
  id: string;
  version?: number;
};

export function versionedEntityArrayChanged<T extends VersionedEntity>(current: T[], next: T[]) {
  if (current.length !== next.length) return true;
  for (let index = 0; index < current.length; index += 1) {
    const left = current[index];
    const right = next[index];
    if (left.id.startsWith("draft-") || right.id.startsWith("draft-")) return true;
    if (left.id !== right.id || (left.version ?? 0) !== (right.version ?? 0)) return true;
  }
  return false;
}

export function replaceVersionedArrayIfChanged<T extends VersionedEntity>(target: T[], next: T[]) {
  if (!versionedEntityArrayChanged(target, next)) return false;
  target.splice(0, target.length, ...next);
  return true;
}
