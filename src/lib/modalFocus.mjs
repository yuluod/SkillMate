const modalStacks = new WeakMap();

export function getModalFocusableElements(modal) {
  if (!modal) return [];
  return Array.from(
    modal.querySelectorAll(
      'a[href], button:not([disabled]), textarea:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])'
    )
  ).filter((element) => !element.hasAttribute("disabled") && !element.getAttribute("aria-hidden"));
}

function focusFirstElement(modal) {
  const focusable = getModalFocusableElements(modal);
  (focusable[0] || modal)?.focus?.();
}

export function activateModalFocus({ document, modal, onClose }) {
  const previousFocus = document.activeElement;
  const stack = modalStacks.get(document) || [];
  const entry = { modal };
  stack.push(entry);
  modalStacks.set(document, stack);

  const isTopModal = () => stack[stack.length - 1] === entry;

  function handleKeyDown(event) {
    if (!isTopModal()) return;
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
      return;
    }
    if (event.key !== "Tab") return;
    const elements = getModalFocusableElements(modal);
    if (elements.length === 0) {
      event.preventDefault();
      modal.focus?.();
      return;
    }
    const first = elements[0];
    const last = elements[elements.length - 1];
    if (!modal.contains(document.activeElement)) {
      event.preventDefault();
      first.focus();
    } else if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  }

  function handleFocusIn(event) {
    if (isTopModal() && !modal.contains(event.target)) {
      focusFirstElement(modal);
    }
  }

  document.addEventListener("keydown", handleKeyDown);
  document.addEventListener("focusin", handleFocusIn);
  focusFirstElement(modal);

  return () => {
    document.removeEventListener("keydown", handleKeyDown);
    document.removeEventListener("focusin", handleFocusIn);
    const index = stack.indexOf(entry);
    const wasTopModal = isTopModal();
    if (index >= 0) stack.splice(index, 1);
    if (stack.length === 0) {
      modalStacks.delete(document);
    }
    if (!wasTopModal) return;
    const nextModal = stack[stack.length - 1]?.modal;
    if (nextModal) {
      focusFirstElement(nextModal);
    } else {
      previousFocus?.focus?.();
    }
  };
}
