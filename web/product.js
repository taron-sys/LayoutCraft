// The product name lives HERE and nowhere else in the web shell: the page title, the ribbon's app name, the About dialog, the PDF producer and
// the palette storage key read it. Renaming the product is a change to this one constant (plus the crate names, which are build-time only).
(function () {
  const product = Object.freeze({ name: 'LayoutCraft', slug: 'layoutcraft' });
  window.layoutcraftProduct = product;
  try { document.title = `${product.name} – Rust/WASM shell`; } catch { /* no document */ }
})();
