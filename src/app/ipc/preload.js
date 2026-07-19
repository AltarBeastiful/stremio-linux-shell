const createIpc = () => {
    let listeners = [];

    globalThis.__postMessage = (data) => {
        listeners.forEach((listener) => {
            listener({ data });
        });
    };

    const postMessage = (data) => {
        globalThis.webkit.messageHandlers.ipc.postMessage(data);
    };

    const addEventListener = (name, listener) => {
        if (name !== 'message')
            throw Error('Unsupported event');

        listeners.push(listener);
    };

    const removeEventListener = (name, listener) => {
        if (name !== 'message')
            throw Error('Unsupported event');

        listeners = listeners.filter((it) => it !== listener);
    };

    return {
        postMessage,
        addEventListener,
        removeEventListener,
    };
};

window.ipc = createIpc();

// Backward compatibility
window.qt = {
    webChannelTransport: {
        send: window.ipc.postMessage,
    },
};

globalThis.chrome = {
    webview: {
        postMessage: window.ipc.postMessage,
        addEventListener: (name, listener) => {
            window.ipc.addEventListener(name, listener);
        },
        removeEventListener: (name, listener) => {
            window.ipc.removeEventListener(name, listener);
        },
    },
};

window.ipc.addEventListener('message', (message) => {
    window.qt.webChannelTransport.onmessage(message);
});

window.stremio_server_ipc_key = "LINUX";

console.log('IPC script injected');

// --- shell_ui: report player-chrome visibility so the shell can freeze the idle
// WebKit overlay (IMPLEMENTATION_PLAN.md / DEVLOG §18). stremio-web adds the
// `overlayHidden` class to `.player-container` when the controls auto-hide while
// playing (immersed && !paused && !menusOpen && !casting). We report a single
// boolean; the shell freezes the overlay only while playback is active AND the
// UI is not visible. Fail-visible + heartbeat: any uncertainty (not on the
// player route, selector drift, an exception) reports *visible*, so a
// stremio-web change degrades to today's CPU behaviour, never to a hidden UI.
(() => {
    const HEARTBEAT_MS = 5000;

    const shown = (el) => !!el && el.offsetParent !== null && el.getClientRects().length > 0;

    const computeVisible = () => {
        const container = document.querySelector('[class*="player-container"]');
        if (!container) return true;                                  // not on the player route
        if (!/overlayHidden/i.test(container.className)) return true; // controls / menu / paused shown
        // Immersed: the control bar and nav bar are hidden. Keep the overlay
        // live only if something that ignores `overlayHidden` is on screen — the
        // buffering spinner or an open menu / next-video popup (all `menu-layer`).
        if (shown(document.querySelector('[class*="buffering"]'))) return true;
        if (shown(document.querySelector('[class*="menu-layer"]'))) return true;
        return false;                                                 // fully immersed → freeze-eligible
    };

    let last = null;
    const report = (force) => {
        let visible;
        try { visible = computeVisible(); } catch (e) { visible = true; } // fail-visible
        if (!force && visible === last) return;
        last = visible;
        try { globalThis.webkit.messageHandlers.shell_ui.postMessage(visible ? 'visible' : 'hidden'); }
        catch (e) { /* handler not registered yet */ }
    };

    let scheduled = false;
    const schedule = () => {
        if (scheduled) return;
        scheduled = true;
        requestAnimationFrame(() => { scheduled = false; report(false); });
    };

    const start = () => {
        // React mutates class names in place; watch the whole tree, coalesced to
        // once per frame so a mutation storm can't spam the IPC channel.
        new MutationObserver(schedule).observe(document.documentElement, {
            subtree: true, childList: true, attributes: true, attributeFilter: ['class'],
        });
        report(true);
        // Heartbeat: re-post current state so the shell can watchdog selector
        // drift (if reports stop while playing, it unfreezes and stays safe).
        setInterval(() => report(true), HEARTBEAT_MS);
    };

    if (document.body) start();
    else document.addEventListener('DOMContentLoaded', start, { once: true });
})();