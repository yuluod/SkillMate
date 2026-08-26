export function SurfaceHeader({ title, description, meta, actions, className = "" }) {
  const hasMeta = meta !== undefined && meta !== null && meta !== "";
  return (
    <header className={`surface-header ${className}`.trim()}>
      <div className="surface-header-copy">
        <div className="surface-title-line">
          <h2>{title}</h2>
          {hasMeta && <span className="surface-meta">{meta}</span>}
        </div>
        {description && <p>{description}</p>}
      </div>
      {actions && <div className="surface-header-actions">{actions}</div>}
    </header>
  );
}

export function SurfaceSectionHeader({ title, description, meta }) {
  const hasMeta = meta !== undefined && meta !== null && meta !== "";
  return (
    <header className="surface-section-head">
      <div>
        <h3>{title}</h3>
        {description && <p>{description}</p>}
      </div>
      {hasMeta && <span className="surface-meta">{meta}</span>}
    </header>
  );
}
