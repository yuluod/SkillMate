import { useEffect, useId, useRef } from "react";
import Icon from "./Icon.jsx";

function getModalFocusableElements(modal) {
  if (!modal) return [];
  return Array.from(
    modal.querySelectorAll(
      'a[href], button:not([disabled]), textarea:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])'
    )
  ).filter((element) => !element.hasAttribute("disabled") && !element.getAttribute("aria-hidden"));
}

export default function ModalShell({ title, icon, className = "", onClose, children }) {
  const titleId = useId();
  const modalRef = useRef(null);

  useEffect(() => {
    const previousFocus = document.activeElement;
    const modal = modalRef.current;
    const focusable = getModalFocusableElements(modal);
    (focusable[0] || modal)?.focus();

    function handleKeyDown(event) {
      if (event.key === "Escape") {
        event.preventDefault();
        onClose();
        return;
      }
      if (event.key !== "Tab") return;
      const elements = getModalFocusableElements(modalRef.current);
      if (elements.length === 0) {
        event.preventDefault();
        modalRef.current?.focus();
        return;
      }
      const first = elements[0];
      const last = elements[elements.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    }

    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("keydown", handleKeyDown);
      previousFocus?.focus?.();
    };
  }, [onClose]);

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div
        ref={modalRef}
        className={`modal ${className}`.trim()}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        onClick={e => e.stopPropagation()}
      >
        <div className="modal-head">
          <h3 id={titleId}>{icon && <Icon name={icon} size={18} />}{title}</h3>
          <button className="modal-x" type="button" aria-label="关闭弹窗" onClick={onClose}>
            <Icon name="x" size={20} />
          </button>
        </div>
        {children}
      </div>
    </div>
  );
}
