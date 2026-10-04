export type CloudSessionView = {
  signedIn: boolean;
  status: "authenticated" | "offline_saved" | "reauth_required";
  userId?: string;
  email?: string;
};
