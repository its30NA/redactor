/**
 * CodeMirror 6 setup for the two panes. Styling lives in `styles/editor.css` (classes only:
 * the shell's CSP has no 'unsafe-inline'). CodeMirror's own theme goes into a constructed style
 * sheet, see `csp-styles.ts`.
 */
import { EditorState, type Extension } from '@codemirror/state';
import {
  drawSelection,
  EditorView,
  highlightSpecialChars,
  keymap,
  lineNumbers,
} from '@codemirror/view';
import { defaultKeymap, history, historyKeymap } from '@codemirror/commands';
import { installCspStyles } from './csp-styles';

installCspStyles();

/**
 * State config for both panes. Only "\n" separates lines, so a CR stays in the document as a
 * character: the document string equals the scanned text, and an editor position is the
 * UTF-16 offset the engine reports (CRLF input).
 */
export function inputState(): Extension {
  return [EditorState.lineSeparator.of('\n')];
}

/** CR renders as nothing (it ends a CRLF line); other invisible characters get a visible dot. */
function renderSpecial(code: number, description: string | null, placeholder: string): HTMLElement {
  const span = document.createElement('span');
  if (code === 13) {
    span.className = 'cm-cr';
    return span;
  }
  span.className = 'cm-special';
  span.textContent = placeholder;
  if (description) {
    span.title = description;
    span.setAttribute('aria-label', description);
  }
  return span;
}

function common(label: string, kind: 'input' | 'output'): Extension {
  return [
    inputState(),
    lineNumbers(),
    highlightSpecialChars({ render: renderSpecial }),
    drawSelection(),
    EditorView.lineWrapping,
    EditorView.editorAttributes.of({ class: `rd-editor rd-${kind}` }),
    EditorView.contentAttributes.of({
      'aria-label': label,
      spellcheck: 'false',
      autocorrect: 'off',
      autocapitalize: 'off',
      autocomplete: 'off',
      translate: 'no',
      'data-gramm': 'false',
    }),
  ];
}

/** The editable Input pane. */
export function inputExtensions(extra: Extension[] = []): Extension {
  return [common('Input text', 'input'), history(), keymap.of([...defaultKeymap, ...historyKeymap]), ...extra];
}

/** The read-only Safe output pane: selectable and copyable, never editable. */
export function outputExtensions(extra: Extension[] = []): Extension {
  return [
    common('Safe output', 'output'),
    EditorState.readOnly.of(true),
    EditorView.editable.of(false),
    EditorView.contentAttributes.of({ tabindex: '0' }),
    keymap.of(defaultKeymap),
    ...extra,
  ];
}
