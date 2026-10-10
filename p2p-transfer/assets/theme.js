// Match the browser's visible chrome to the egui theme selected in the app.
const CHROME = {
    clean: {
        light: { header: "#f4f5f7", background: "#e4e4e4" },
        dark: { header: "#23263a", background: "#151724" },
    },
    rusty: {
        light: { header: "#edded4", background: "#faf6f2" },
        dark: { header: "#231b17", background: "#100d0c" },
    },
    phosphor: {
        light: { header: "#dcecd2", background: "#f1f7ec" },
        dark: { header: "#0a1a0c", background: "#030a04" },
    },
};

export function setBrowserTheme(theme, dark) {
    const colors = (CHROME[theme] ?? CHROME.clean)[dark ? "dark" : "light"];
    document.querySelectorAll('meta[name="theme-color"]').forEach(meta => {
        meta.setAttribute("content", colors.header);
    });
    document.body.style.backgroundColor = colors.background;
}
