import { ref, type ObjectDirective } from "vue";

const hoverNavigationEnabled = ref(false);
const hoverHandlers = new WeakMap<HTMLElement, () => void>();

export function setHoverNavigationEnabled(enabled: boolean) {
  hoverNavigationEnabled.value = enabled;
}

export const hoverSwitchDirective: ObjectDirective<HTMLElement, (() => void) | undefined> = {
  mounted(element, binding) {
    const handleMouseEnter = () => {
      if (!hoverNavigationEnabled.value) return;
      if (element instanceof HTMLButtonElement && element.disabled) return;
      if (element.classList.contains("active") || element.getAttribute("aria-selected") === "true" || element.hasAttribute("aria-current")) return;
      if (binding.value) binding.value();
      else element.click();
    };
    hoverHandlers.set(element, handleMouseEnter);
    element.addEventListener("mouseenter", handleMouseEnter);
  },
  unmounted(element) {
    const handleMouseEnter = hoverHandlers.get(element);
    if (!handleMouseEnter) return;
    element.removeEventListener("mouseenter", handleMouseEnter);
    hoverHandlers.delete(element);
  },
};
