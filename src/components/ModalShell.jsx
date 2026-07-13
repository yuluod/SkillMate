import { useEffect, useId, useRef } from "react";
import Icon from "./Icon.jsx";
import { activateModalFocus } from "../lib/modalFocus.mjs";

export default function ModalShell({ title, icon, className = "", onClose, children, role = "dialog", descriptionId }) {
  const titleId = useId();
  const modalRef = useRef(null);
  const onCloseRef = useRef(onClose);

  useEffect(() => {
    onCloseRef.current = onClose;
  }, [onClose]);

  useEffect(() => {
    const modal = modalRef.current;
    if (!modal) return undefined;
    return activateModalFocus({
      document,
      modal,
      onClose: () => onCloseRef.current(),
    });
  }, []);

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div
        ref={modalRef}
        className={`modal ${className}`.trim()}
        role={role}
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={descriptionId}
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
