//! The static parts of the web page the export writes, copied from the template string of `web/webpage.js` by `visio-test-harness/gen-webpage-template.py`
//! (the style sheet and the tab script); the oracle test (`tests/webpage_oracle.rs`) fails when they differ from the web's.
pub const HEAD: &str = r####"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>"####;
pub const AFTER_TITLE: &str = r####"</title>
<style>body{margin:0;font:14px system-ui,sans-serif;background:#f3f3f3}nav{display:flex;gap:2px;padding:6px 8px 0;background:#e6e6e6;flex-wrap:wrap}nav button{border:1px solid #bbb;border-bottom:0;background:#ddd;padding:5px 12px;cursor:pointer}nav button[aria-selected=true]{background:#fff;font-weight:600}main{padding:12px;overflow:auto}section svg{max-width:100%;height:auto;background:#fff;box-shadow:0 1px 4px #0003}svg a{cursor:pointer}svg a:hover{opacity:.85}svg text{user-select:text}</style></head><body>
<nav role="tablist">"####;
pub const AFTER_NAV: &str = r####"</nav>
<main>"####;
pub const TAIL: &str = r####"</main>
<script>(function(){function show(i){document.querySelectorAll('nav button').forEach(function(x){x.setAttribute('aria-selected',String(x.getAttribute('data-i')===String(i)))});document.querySelectorAll('section').forEach(function(s){s.hidden=s.getAttribute('data-i')!==String(i)})}document.querySelector('nav').addEventListener('click',function(e){var b=e.target.closest('button');if(b)show(b.getAttribute('data-i'))});document.querySelector('main').addEventListener('click',function(e){var a=e.target.closest('a[data-page]');if(a){e.preventDefault();show(a.getAttribute('data-page'))}})})();</script>
</body></html>
"####;
