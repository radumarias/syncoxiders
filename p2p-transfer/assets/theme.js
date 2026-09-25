// Match the browser's visible chrome to the egui theme selected in the app.
export function setBrowserTheme(theme, dark) {
    const colors = theme === "clean"
        ? (dark ? { header: "#1d2835", background: "#0e1621" }
            : { header: "#f8fafc", background: "#e5ebf1" })
        : (dark ? { header: "#231b17", background: "#100d0c" }
            : { header: "#fffdfb", background: "#faf6f2" });
    document.querySelectorAll('meta[name="theme-color"]').forEach(meta => {
        meta.setAttribute("content", colors.header);
    });
    document.body.style.backgroundColor = colors.background;
}
