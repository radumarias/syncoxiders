// Match the browser's visible chrome to the egui theme selected in the app.
export function setBrowserTheme(theme, dark) {
    const colors = theme === "clean"
        ? (dark ? { header: "#23263a", background: "#151724" }
            : { header: "#f4f5f7", background: "#e4e4e4" })
        : (dark ? { header: "#231b17", background: "#100d0c" }
            : { header: "#fffdfb", background: "#faf6f2" });
    document.querySelectorAll('meta[name="theme-color"]').forEach(meta => {
        meta.setAttribute("content", colors.header);
    });
    document.body.style.backgroundColor = colors.background;
}
