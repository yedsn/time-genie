import Sortable, { MultiDrag, type SortableEvent } from "sortablejs";
import { planTaskMoveIntent, taskDragTargetDepth, taskDragTargetInvalid, type PlanTaskMoveIntent, type PlanTaskTreeItem } from "./planTaskInteractionCore";

const sortableWithPlugins = Sortable as typeof Sortable & { timeGenieTreeMultiDragMounted?: boolean };
if (!sortableWithPlugins.timeGenieTreeMultiDragMounted) {
  Sortable.mount(new MultiDrag());
  sortableWithPlugins.timeGenieTreeMultiDragMounted = true;
}

export type TaskTreeSortableCoordinator = {
  destroy: () => void;
  mount: () => void;
  prepareSubtree: (taskId: string) => void;
};

export function createTaskTreeSortable(options: {
  container: () => HTMLElement | null | undefined;
  rowSelector: string;
  handleSelector: string;
  taskIdAttribute: string;
  disabled?: () => boolean;
  subtreeIds: (taskId: string) => ReadonlySet<string>;
  parentId: (taskId: string) => string | undefined;
  onStart?: () => void;
  onMove?: (depth: 0 | 1, invalid: boolean) => void;
  onIntent: (taskId: string, intent: PlanTaskMoveIntent) => void | Promise<void>;
  onInvalid?: () => void | Promise<void>;
  onSettled?: () => void | Promise<void>;
}): TaskTreeSortableCoordinator {
  let sortable: Sortable | undefined;
  let targetDepth: 0 | 1 = 0;

  const rows = () => Array.from(options.container()?.querySelectorAll<HTMLElement>(options.rowSelector) ?? []);
  const taskIdOf = (element: HTMLElement) => element.getAttribute(options.taskIdAttribute) ?? "";

  function clearSelection(elements = rows()) {
    for (const element of elements) {
      if (element.isConnected && element.parentElement) Sortable.utils.deselect(element);
    }
  }

  function clearDropState() {
    const container = options.container();
    container?.removeAttribute("data-drop-depth");
    container?.classList.remove("drop-invalid");
  }

  async function settle(elements: HTMLElement[]) {
    clearSelection(elements);
    clearDropState();
    await options.onSettled?.();
  }

  function prepareSubtree(taskId: string) {
    if (options.disabled?.()) return;
    const selected = options.subtreeIds(taskId);
    const elements = rows();
    clearSelection(elements);
    for (const element of elements) if (selected.has(taskIdOf(element))) Sortable.utils.select(element);
  }

  function destroy() {
    if (sortable) {
      const current = sortable;
      sortable = undefined;
      if (current.el?.isConnected) current.destroy();
    }
    clearDropState();
  }

  async function finish(event: SortableEvent) {
    const taskId = taskIdOf(event.item);
    const elements = rows();
    if (!taskId || event.oldIndex === undefined || event.newIndex === undefined) {
      await settle(elements);
      return;
    }
    const movingIds = options.subtreeIds(taskId);
    const orderedRows: PlanTaskTreeItem[] = elements.map((element) => {
      const id = taskIdOf(element);
      return { id, parentId: options.parentId(id) };
    });
    const intent = planTaskMoveIntent(orderedRows, taskId, movingIds, targetDepth, movingIds.size > 1);
    try {
      if (intent) await options.onIntent(taskId, intent);
      else await options.onInvalid?.();
    } finally {
      await settle(elements);
    }
  }

  function mount() {
    destroy();
    const container = options.container();
    if (!container || options.disabled?.()) return;
    sortable = new Sortable(container, {
      animation: 140,
      draggable: options.rowSelector,
      handle: options.handleSelector,
      forceFallback: true,
      fallbackOnBody: true,
      multiDrag: true,
      selectedClass: "sortable-tree-selected",
      avoidImplicitDeselect: true,
      ghostClass: "sortable-ghost",
      chosenClass: "sortable-chosen",
      dragClass: "sortable-drag",
      onStart: options.onStart,
      onMove(event, originalEvent) {
        const related = event.related as HTMLElement;
        const pointerX = originalEvent instanceof MouseEvent ? originalEvent.clientX : 0;
        const listLeft = container.getBoundingClientRect().left;
        targetDepth = taskDragTargetDepth(Number(related?.dataset.depth ?? 0), pointerX, listLeft);
        const taskId = taskIdOf(event.dragged as HTMLElement);
        const invalid = taskDragTargetInvalid(targetDepth, options.subtreeIds(taskId));
        container.setAttribute("data-drop-depth", String(targetDepth));
        container.classList.toggle("drop-invalid", invalid);
        options.onMove?.(targetDepth, invalid);
        return !invalid;
      },
      onEnd(event) { void finish(event); },
    });
  }

  return { destroy, mount, prepareSubtree };
}
