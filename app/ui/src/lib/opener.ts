/**
 * The element a sheet or dialog hands focus back to when it closes.
 *
 * Opened by a click: the button clicked. WebKit on a Mac does not focus a button that is
 * clicked, so the focused element is not it there. The click is remembered only until its
 * task ends: a sheet opened later by a shortcut never gets an old button. Opened by a key: the
 * focused element.
 */

const FOCUSABLE = 'button, [href], input, select, textarea, [tabindex]';

let justClicked: HTMLElement | null = null;

if (typeof window !== 'undefined') {
  window.addEventListener(
    'click',
    (e) => {
      const target = e.target instanceof Element ? e.target.closest<HTMLElement>(FOCUSABLE) : null;
      if (!target) return;
      justClicked = target;
      // Effects run in a microtask after the click handler; a timer runs after them.
      setTimeout(() => {
        if (justClicked === target) justClicked = null;
      }, 0);
    },
    true,
  );
}

/** The button just clicked (in this task), else the focused element, else nothing. */
export function openerNow(): HTMLElement | null {
  if (justClicked?.isConnected) return justClicked;
  const focused = document.activeElement;
  return focused instanceof HTMLElement && focused !== document.body ? focused : null;
}
