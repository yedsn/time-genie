import { ref, type ObjectDirective } from "vue";
import { createHoverFrameCommitter } from "./hoverNavigationCore";
import { recordWorkbenchNavigationMetric } from "./workbenchNavigationMetrics";

const hoverNavigationEnabled = ref(false);
const hoverHandlers = new WeakMap<HTMLElement, { enter: () => void; click: () => void }>();
const committer = createHoverFrameCommitter({
  request: (callback) => window.requestAnimationFrame(callback),
  cancel: (handle) => window.cancelAnimationFrame(handle),
});

export function setHoverNavigationEnabled(enabled: boolean) {
  hoverNavigationEnabled.value = enabled;
}

export const hoverSwitchDirective: ObjectDirective<HTMLElement, (() => void) | undefined> = {
  mounted(element, binding) {
    const handleMouseEnter = () => {
      if (!hoverNavigationEnabled.value) return;
      if (element instanceof HTMLButtonElement && element.disabled) return;
      if (element.classList.contains("active") || element.getAttribute("aria-selected") === "true" || element.hasAttribute("aria-current")) return;
      recordWorkbenchNavigationMetric("hoverScheduled");
      committer.schedule(() => {
        if (!element.isConnected) return;
        if (element instanceof HTMLButtonElement && element.disabled) return;
        if (element.classList.contains("active") || element.getAttribute("aria-selected") === "true" || element.hasAttribute("aria-current")) return;
        recordWorkbenchNavigationMetric("hoverCommitted");
        if (binding.value) binding.value();
        else element.click();
      });
    };
    const handleClick = () => {
      committer.cancel();
      recordWorkbenchNavigationMetric("hoverCancelled");
    };
    hoverHandlers.set(element, { enter: handleMouseEnter, click: handleClick });
    element.addEventListener("mouseenter", handleMouseEnter);
    element.addEventListener("click", handleClick);
  },
  unmounted(element) {
    const handlers = hoverHandlers.get(element);
    if (!handlers) return;
    element.removeEventListener("mouseenter", handlers.enter);
    element.removeEventListener("click", handlers.click);
    hoverHandlers.delete(element);
  },
};
