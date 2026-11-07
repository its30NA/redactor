/**
 * CodeMirror mounts its themes through style-mod, which writes them into a <style> element in
 * <head> when the root is a document. The shell's CSP (style-src 'self', no 'unsafe-inline')
 * refuses that element and the editor would render unstyled. A constructed style sheet is
 * CSSOM, which the CSP allows, so the same modules go into one adopted sheet instead, in the
 * order style-mod would keep them (later themes override earlier ones).
 */
import { StyleModule } from 'style-mod';

let installed = false;

export function installCspStyles(): void {
  if (installed || typeof document === 'undefined' || typeof CSSStyleSheet === 'undefined') return;
  installed = true;
  const modules: StyleModule[] = [];
  const sheet = new CSSStyleSheet();

  StyleModule.mount = (root, mods) => {
    const list: readonly StyleModule[] = Array.isArray(mods) ? mods : [mods as StyleModule];
    let j = 0;
    let changed = false;
    for (const mod of list) {
      let index = modules.indexOf(mod);
      if (index < j && index > -1) {
        // Ordering conflict: move it after what it must follow.
        modules.splice(index, 1);
        j--;
        index = -1;
        changed = true;
      }
      if (index === -1) {
        modules.splice(j++, 0, mod);
        changed = true;
      } else {
        j = index + 1;
      }
    }
    if (changed) sheet.replaceSync(modules.map((m) => m.getRules()).join('\n'));
    const target = (root as Document).adoptedStyleSheets ? (root as Document) : document;
    if (!target.adoptedStyleSheets.includes(sheet)) target.adoptedStyleSheets = [sheet, ...target.adoptedStyleSheets];
  };
}
