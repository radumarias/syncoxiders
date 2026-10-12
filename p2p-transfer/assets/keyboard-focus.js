// eframe answers every touchend by blurring its hidden text input and focusing it again,
// so that iOS shows the keyboard. Android closes the keyboard on that blur and opens it
// again on the focus, which makes the page jump on every tap while typing. Outside iOS,
// skip a blur of a focused text input while a touchend is being dispatched; eframe's
// own blur when editing ends runs in an animation frame and is unaffected.
(() => {
    const ios = /iPad|iPhone|iPod/.test(navigator.userAgent)
        || (navigator.platform === 'MacIntel' && navigator.maxTouchPoints > 1);
    if (ios) return;
    let inTouchEnd = false;
    window.addEventListener('touchend', () => {
        inTouchEnd = true;
        setTimeout(() => { inTouchEnd = false; }, 0);
    }, { capture: true, passive: true });
    const blur = HTMLInputElement.prototype.blur;
    HTMLInputElement.prototype.blur = function () {
        if (inTouchEnd && document.activeElement === this) return;
        blur.call(this);
    };
})();
