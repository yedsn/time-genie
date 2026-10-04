export type CloudDeviceView = {
  id: string;
  deviceName: string;
  platform: string;
  appVersion: string;
  lastSeenAt: string;
  current: boolean;
  revokedAt?: string;
};
