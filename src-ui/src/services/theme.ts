export const APP_THEMES = [
  {
    id: "forest",
    name: "森林",
    description: "柔和绿意，当前默认",
    swatches: ["#101513", "#18201d", "#c9ef8f"],
    mode: "dark",
  },
  {
    id: "graphite",
    name: "石墨",
    description: "克制中性，专注内容",
    swatches: ["#111315", "#1b1e21", "#aeb8c4"],
    mode: "dark",
  },
  {
    id: "ocean",
    name: "深海",
    description: "冷静蓝调，层次清晰",
    swatches: ["#0b1218", "#111e28", "#79c7ff"],
    mode: "dark",
  },
  {
    id: "ember",
    name: "暖夜",
    description: "暖色强调，低光舒适",
    swatches: ["#16110f", "#241b17", "#f0b36d"],
    mode: "dark",
  },
  {
    id: "paper",
    name: "纸白",
    description: "明亮纯净，适合白天",
    swatches: ["#f4f5f2", "#ffffff", "#4f7f46"],
    mode: "light",
  },
  {
    id: "mist",
    name: "雾蓝",
    description: "清爽浅蓝，柔和低对比",
    swatches: ["#eef4f8", "#ffffff", "#3f76a8"],
    mode: "light",
  },
] as const;

export type AppTheme = (typeof APP_THEMES)[number]["id"];

const APP_THEME_CACHE_KEY = "timegenie:app-theme";

export function isAppTheme(value: unknown): value is AppTheme {
  return APP_THEMES.some((theme) => theme.id === value);
}

export function applyAppTheme(theme: AppTheme, cache = false) {
  document.documentElement.dataset.theme = theme;
  const selected = APP_THEMES.find((item) => item.id === theme);
  document.documentElement.style.colorScheme = selected?.mode ?? "dark";
  if (cache) {
    try {
      window.localStorage.setItem(APP_THEME_CACHE_KEY, theme);
    } catch {
      // The persisted device setting remains the source of truth when storage is unavailable.
    }
  }
}

export function getCachedAppTheme(): AppTheme {
  try {
    const cached = window.localStorage.getItem(APP_THEME_CACHE_KEY);
    return isAppTheme(cached) ? cached : "forest";
  } catch {
    return "forest";
  }
}
