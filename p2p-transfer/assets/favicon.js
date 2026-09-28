// Use distinct URLs: favicon renderers can ignore SVG media queries or cache
// their light rendering. Follow the browser preference, not the canvas theme.
(() => {
    const icon = document.getElementById("favicon");
    const scheme = window.matchMedia("(prefers-color-scheme: dark)");
    const update = () => {
        icon.href = scheme.matches ? icon.dataset.dark : icon.dataset.light;
    };
    update();
    scheme.addEventListener("change", update);
})();
