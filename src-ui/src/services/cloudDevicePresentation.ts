import type { CloudDeviceView } from "./cloudDevicePresentationTypes";

export function cloudDeviceKind(device: CloudDeviceView) {
  return device.current ? "当前设备" : device.platform;
}

export function cloudDeviceActionLabel(device: CloudDeviceView) {
  return device.current ? "退出" : "撤销";
}

export function cloudDeviceActionDisabled(device: CloudDeviceView, working: boolean) {
  return working || Boolean(device.revokedAt);
}

export async function runConfirmedCloudAction<T>(
  confirmAction: () => Promise<unknown>,
  action: () => Promise<T>,
): Promise<T | undefined> {
  try {
    await confirmAction();
  } catch {
    return undefined;
  }
  return await action();
}
