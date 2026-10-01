import confetti from "canvas-confetti";
import { Howl } from "howler";
import completionSuccessSoundUrl from "../assets/audio/completion-success.wav";
import { CompletionFeedbackGate, completionFeedbackMessage, decideCompletionFeedbackMode, runCompletionEffect, type CompletionFeedbackMode } from "./completionFeedbackCore";

export type CompletionFeedbackRequest = {
  completedCount: number;
  openBefore?: number;
  openAfter?: number;
};

export type CompletionFeedbackOutcome = {
  mode: CompletionFeedbackMode;
  displayed: boolean;
};

type FeedbackBannerElements = {
  root: HTMLDivElement;
  eyebrow: HTMLSpanElement;
  title: HTMLElement;
  detail: HTMLSpanElement;
};

type ConfettiCannon = ReturnType<typeof confetti.create>;
type SideBurst = {
  particleCount: number;
  velocity: [number, number];
  spread: [number, number];
  originY: [number, number];
  scalar: [number, number];
  ticks: [number, number];
};

let completionFeedbackEnabled = true;
let completionFeedbackSoundEnabled = true;
let feedbackCanvas: HTMLCanvasElement | undefined;
let fireConfetti: ReturnType<typeof confetti.create> | undefined;
let particleShapes: confetti.Shape[] | undefined;
let feedbackBanner: FeedbackBannerElements | undefined;
let bannerTimer: number | undefined;
let completionSound: Howl | undefined;
const particleTimers = new Set<number>();
const feedbackGate = new CompletionFeedbackGate();

export function setCompletionFeedbackEnabled(enabled: boolean) {
  completionFeedbackEnabled = enabled;
  if (enabled) return;
  clearActiveFeedback();
}

export function isCompletionFeedbackEnabled() {
  return completionFeedbackEnabled;
}

export function setCompletionFeedbackSoundEnabled(enabled: boolean) {
  completionFeedbackSoundEnabled = enabled;
  if (!enabled) stopCompletionSound();
}

export function isCompletionFeedbackSoundEnabled() {
  return completionFeedbackSoundEnabled;
}

export function stopCompletionFeedback() {
  clearActiveFeedback();
  stopCompletionSound();
  completionSound?.unload();
  completionSound = undefined;
  fireConfetti = undefined;
  feedbackCanvas?.remove();
  feedbackCanvas = undefined;
  feedbackBanner?.root.remove();
  feedbackBanner = undefined;
}

export function playCompletionFeedback(request: CompletionFeedbackRequest): CompletionFeedbackOutcome {
  const mode = decideCompletionFeedbackMode({ ...request, enabled: completionFeedbackEnabled });
  if (mode === "none") return { mode, displayed: false };

  showFeedbackBanner(mode, request.completedCount);
  if (feedbackGate.shouldLaunch(mode, Date.now())) {
    const launched = runCompletionEffect(() => launchConfetti(mode));
    if (!launched) console.warn("完成庆祝粒子不可用");
    if (completionFeedbackSoundEnabled) {
      const sounded = runCompletionEffect(() => playCompletionSound(mode));
      if (!sounded) console.warn("完成激励音效不可用");
    }
  }
  return { mode, displayed: true };
}

function clearActiveFeedback() {
  if (bannerTimer) window.clearTimeout(bannerTimer);
  bannerTimer = undefined;
  for (const timer of particleTimers) window.clearTimeout(timer);
  particleTimers.clear();
  feedbackBanner?.root.classList.remove("visible");
  fireConfetti?.reset();
  stopCompletionSound();
}

function playCompletionSound(mode: Exclude<CompletionFeedbackMode, "none">) {
  const sound = ensureCompletionSound();
  sound.stop();
  sound.volume(mode === "all-clear" ? 0.72 : 0.56);
  sound.rate(1);
  sound.play();
}

function ensureCompletionSound() {
  completionSound ??= new Howl({
    src: [completionSuccessSoundUrl],
    format: ["wav"],
    preload: true,
    html5: false,
    pool: 1,
  });
  return completionSound;
}

function stopCompletionSound() {
  completionSound?.stop();
}

function scheduleParticleBurst(callback: () => void, delay: number) {
  const timer = window.setTimeout(() => {
    particleTimers.delete(timer);
    callback();
  }, delay);
  particleTimers.add(timer);
}

function ensureConfetti() {
  if (fireConfetti) return fireConfetti;
  feedbackCanvas = document.createElement("canvas");
  feedbackCanvas.className = "completion-confetti-canvas";
  feedbackCanvas.setAttribute("aria-hidden", "true");
  document.body.append(feedbackCanvas);
  fireConfetti = confetti.create(feedbackCanvas, { resize: true, useWorker: true, disableForReducedMotion: false });
  return fireConfetti;
}

function launchConfetti(mode: Exclude<CompletionFeedbackMode, "none">) {
  const fire = ensureConfetti();
  const colors = ["#d9ff9f", "#91f5d4", "#ffd27a", "#f8fff1", "#8bb8ff"];
  const shapes = ensureParticleShapes();

  if (mode === "all-clear") {
    fire.reset();
    launchSidePair(fire, colors, shapes, { particleCount: 48, velocity: [64, 76], spread: [18, 28], originY: [0.9, 0.98], scalar: [0.82, 1.16], ticks: [210, 250] });
    scheduleParticleBurst(() => launchSidePair(fire, colors, shapes, { particleCount: 34, velocity: [58, 70], spread: [22, 34], originY: [0.84, 0.96], scalar: [0.72, 1.08], ticks: [195, 235] }), 170);
    scheduleParticleBurst(() => launchSidePair(fire, colors, shapes, { particleCount: 22, velocity: [52, 64], spread: [28, 40], originY: [0.88, 0.99], scalar: [0.62, 0.94], ticks: [180, 220] }), 380);
    return;
  }

  launchSidePair(fire, colors, shapes, { particleCount: 28, velocity: [58, 70], spread: [18, 28], originY: [0.88, 0.98], scalar: [0.72, 1.04], ticks: [180, 220] });
  scheduleParticleBurst(() => launchSidePair(fire, colors, shapes, { particleCount: 18, velocity: [50, 62], spread: [24, 36], originY: [0.82, 0.95], scalar: [0.62, 0.92], ticks: [165, 205] }), 150);
}

function ensureParticleShapes() {
  if (particleShapes) return particleShapes;
  try {
    const kite = confetti.shapeFromPath({
      path: "M50 0 L96 38 L72 100 L10 76 Z",
      matrix: new DOMMatrix([0.1, 0, 0, 0.1, -5, -5]),
    });
    const ribbon = confetti.shapeFromPath({
      path: "M0 18 C26 0 72 2 100 22 L76 50 L100 82 C70 100 30 98 0 78 L24 48 Z",
      matrix: new DOMMatrix([0.1, 0, 0, 0.1, -5, -5]),
    });
    const bolt = confetti.shapeFromPath({
      path: "M58 0 L16 56 H46 L34 100 L86 40 H56 Z",
      matrix: new DOMMatrix([0.1, 0, 0, 0.1, -5, -5]),
    });
    const heart = confetti.shapeFromPath({
      path: "M167 72 C186 34 204 16 242 16 C284 16 318 49 318 91 C318 167 242 242 167 318 C91 242 16 167 16 91 C16 49 49 16 91 16 C129 16 148 34 167 72 Z",
      matrix: new DOMMatrix([0.03333333333333333, 0, 0, 0.03333333333333333, -5.566666666666666, -5.533333333333333]),
    });
    particleShapes = ["square", "square", "circle", "star", kite, ribbon, bolt, heart];
  } catch {
    particleShapes = ["square", "square", "circle", "star"];
  }
  return particleShapes;
}

function launchSidePair(fire: ConfettiCannon, colors: string[], shapes: confetti.Shape[], burst: SideBurst) {
  launchSideBurst(fire, "left", colors, shapes, burst);
  launchSideBurst(fire, "right", colors, shapes, burst);
}

function launchSideBurst(fire: ConfettiCannon, side: "left" | "right", colors: string[], shapes: confetti.Shape[], burst: SideBurst) {
  const fromLeft = side === "left";
  const upwardInwardAngle = randomBetween(38, 48);
  void fire({
    particleCount: Math.round(burst.particleCount * randomBetween(0.86, 1.14)),
    angle: fromLeft ? upwardInwardAngle : 180 - upwardInwardAngle,
    spread: randomBetween(...burst.spread),
    startVelocity: randomBetween(...burst.velocity),
    decay: randomBetween(0.92, 0.95),
    gravity: randomBetween(0.38, 0.62),
    drift: fromLeft ? randomBetween(0.04, 0.18) : randomBetween(-0.18, -0.04),
    scalar: randomBetween(...burst.scalar),
    ticks: Math.round(randomBetween(...burst.ticks)),
    origin: { x: fromLeft ? randomBetween(0.005, 0.035) : randomBetween(0.965, 0.995), y: randomBetween(...burst.originY) },
    colors,
    shapes,
  });
}

function randomBetween(min: number, max: number) {
  return min + Math.random() * (max - min);
}

function feedbackDetail(mode: CompletionFeedbackMode, completedCount: number) {
  if (mode === "all-clear") {
    return completedCount > 1 ? "本次完成 " + completedCount + " 项，当前清单已经清空" : "当前清单已经清空，给自己一点掌声";
  }
  return completedCount > 1 ? "一口气推进了重要进度，状态很棒" : "节奏很好，下一项也会更轻松";
}

function showFeedbackBanner(mode: CompletionFeedbackMode, completedCount: number) {
  feedbackBanner ??= createFeedbackBanner();
  feedbackBanner.eyebrow.textContent = mode === "all-clear" ? "全部完成" : completedCount > 1 ? "批量完成" : "完成确认";
  feedbackBanner.title.textContent = completionFeedbackMessage(mode, completedCount);
  feedbackBanner.detail.textContent = feedbackDetail(mode, completedCount);
  feedbackBanner.root.dataset.mode = mode;
  feedbackBanner.root.setAttribute("aria-label", String(feedbackBanner.title.textContent) + "，" + String(feedbackBanner.detail.textContent));
  feedbackBanner.root.classList.remove("visible");
  void feedbackBanner.root.offsetWidth;
  feedbackBanner.root.classList.add("visible");
  if (bannerTimer) window.clearTimeout(bannerTimer);
  bannerTimer = window.setTimeout(() => feedbackBanner?.root.classList.remove("visible"), mode === "all-clear" ? 3200 : 2400);
}

function createFeedbackBanner(): FeedbackBannerElements {
  const root = document.createElement("div");
  root.className = "completion-feedback-banner";
  root.setAttribute("role", "status");
  root.setAttribute("aria-live", "polite");
  root.setAttribute("aria-atomic", "true");

  const ambient = document.createElement("span");
  ambient.className = "completion-feedback-ambient";
  ambient.setAttribute("aria-hidden", "true");

  const icon = document.createElement("span");
  icon.className = "completion-feedback-icon";
  icon.setAttribute("aria-hidden", "true");
  icon.innerHTML = '<span class="completion-feedback-check">✓</span><span class="completion-feedback-spark spark-one">✦</span><span class="completion-feedback-spark spark-two">✧</span>';

  const copy = document.createElement("span");
  copy.className = "completion-feedback-copy";
  const eyebrow = document.createElement("span");
  eyebrow.className = "completion-feedback-eyebrow";
  const title = document.createElement("strong");
  title.className = "completion-feedback-title";
  const detail = document.createElement("span");
  detail.className = "completion-feedback-detail";
  copy.append(eyebrow, title, detail);

  const flourish = document.createElement("span");
  flourish.className = "completion-feedback-flourish";
  flourish.setAttribute("aria-hidden", "true");
  flourish.innerHTML = "<i></i><i></i><i></i>";

  root.append(ambient, icon, copy, flourish);
  document.body.append(root);
  return { root, eyebrow, title, detail };
}
