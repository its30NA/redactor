// Run by the shell at document start (first_frame.rs `page_theme_script`; `__THEME__` is the
// chosen theme). The webview may not have created <html> yet, so wait for it: mutation observers
// run as a microtask, before the first paint. Tested in app/ui/tests/unit/page-theme-script.test.ts.
(function () {
  function apply() {
    var root = document.documentElement;
    if (!root) return false;
    root.dataset.theme = '__THEME__';
    return true;
  }
  if (apply()) return;
  new MutationObserver(function (_records, observer) {
    if (apply()) observer.disconnect();
  }).observe(document, { childList: true });
})();
