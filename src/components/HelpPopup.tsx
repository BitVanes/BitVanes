/**
 * Reusable help popup: a `?` icon that toggles a small explanation panel.
 * Used throughout the config bar to explain each knob's effect on the
 * pipeline output.
 */

import { useState, type ReactElement, type ReactNode } from 'react';

interface HelpPopupProps {
  children: ReactNode;
}

export function HelpPopup({ children }: HelpPopupProps): ReactElement {
  const [open, setOpen] = useState(false);
  return (
    <span className="help-popup">
      <button
        className="help-icon"
        onClick={(e) => {
          e.preventDefault();
          setOpen(!open);
        }}
        aria-label="Help"
        type="button"
      >
        ?
      </button>
      {open && (
        <>
          <div className="help-overlay" onClick={() => setOpen(false)} />
          <div className="help-content">{children}</div>
        </>
      )}
    </span>
  );
}
