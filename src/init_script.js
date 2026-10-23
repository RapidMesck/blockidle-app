(function() {
    'use strict';
    if (window.__BLOCKIDLE_CLIENT_INITIALIZED__) {
        return;
    }
    window.__BLOCKIDLE_CLIENT_INITIALIZED__ = true;
    window.__BLOCKIDLE_CLIENT_STATE__ = __BLOCKIDLE_INITIAL_STATE_JSON__;

    const nativeRaf = window.requestAnimationFrame
        ? window.requestAnimationFrame.bind(window)
        : function(cb) { return window.setTimeout(function() { cb(performance.now()); }, 16); };
    const nativeCancelRaf = window.cancelAnimationFrame
        ? window.cancelAnimationFrame.bind(window)
        : function(id) { window.clearTimeout(id); };
    const nativeSetTimeout = window.setTimeout.bind(window);
    const nativeClearTimeout = window.clearTimeout.bind(window);

    let nextRequestId = 1;
    let pendingCallbacks = [];
    let activeBatch = null;
    let scheduledNativeRafId = null;
    let scheduledTimerId = null;
    let lastFrameTimestamp = -Infinity;
    let deliveredFrameTimestamps = [];
    let liveFps = 0;

    function isKeepAwakeEnabled() {
        const st = window.__BLOCKIDLE_CLIENT_STATE__;
        return Boolean(st && st.settings && st.settings.keep_awake);
    }

    const docProto = (typeof Document !== 'undefined' && Document.prototype)
        ? Document.prototype
        : Object.getPrototypeOf(document);
    const nativeHiddenDescriptor =
        Object.getOwnPropertyDescriptor(document, 'hidden') ||
        (docProto ? Object.getOwnPropertyDescriptor(docProto, 'hidden') : null);
    const nativeVisibilityStateDescriptor =
        Object.getOwnPropertyDescriptor(document, 'visibilityState') ||
        (docProto ? Object.getOwnPropertyDescriptor(docProto, 'visibilityState') : null);

    try {
        Object.defineProperty(document, 'hidden', {
            configurable: true,
            enumerable: true,
            get: function() {
                if (isKeepAwakeEnabled()) {
                    return false;
                }
                if (nativeHiddenDescriptor && typeof nativeHiddenDescriptor.get === 'function') {
                    return nativeHiddenDescriptor.get.call(document);
                }
                return false;
            }
        });
        Object.defineProperty(document, 'visibilityState', {
            configurable: true,
            enumerable: true,
            get: function() {
                if (isKeepAwakeEnabled()) {
                    return 'visible';
                }
                if (nativeVisibilityStateDescriptor && typeof nativeVisibilityStateDescriptor.get === 'function') {
                    return nativeVisibilityStateDescriptor.get.call(document);
                }
                return 'visible';
            }
        });
    } catch (_err) {}

    function suppressVisibilityChange(event) {
        if (isKeepAwakeEnabled()) {
            if (event && typeof event.stopImmediatePropagation === 'function') {
                event.stopImmediatePropagation();
            }
            if (event && typeof event.stopPropagation === 'function') {
                event.stopPropagation();
            }
        }
    }
    document.addEventListener('visibilitychange', suppressVisibilityChange, true);
    window.addEventListener('visibilitychange', suppressVisibilityChange, true);

    function getEffectiveCap() {
        const st = window.__BLOCKIDLE_CLIENT_STATE__;
        if (!st) {
            return null;
        }
        if (typeof st.effective_fps_cap === 'number') {
            return st.effective_fps_cap;
        }
        return null;
    }

    let hudElement = null;
    let settingsOverlayElement = null;
    let fgSelectElement = null;
    let bgSelectElement = null;
    let toggleElements = {};
    let keybindButtons = {};
    let keybindStatusElement = null;
    let recordingAction = null;
    let keybindStatusText = '';
    const consumedKeys = new Set();

    const KEYBIND_ACTIONS = [
        { key: 'toggle_settings_overlay', label: 'Toggle Settings Overlay' },
        { key: 'toggle_fps_hud', label: 'Toggle FPS HUD' },
        { key: 'reload_game_view', label: 'Reload Game View' },
        { key: 'hide_to_tray', label: 'Hide to System Tray' },
        { key: 'toggle_audio_mute', label: 'Toggle Audio Mute' },
        { key: 'toggle_fullscreen', label: 'Toggle Fullscreen' }
    ];

    const DEFAULT_KEYBINDINGS = {
        toggle_settings_overlay: 'F2',
        toggle_fps_hud: 'F3',
        reload_game_view: 'F5',
        hide_to_tray: 'F9',
        toggle_audio_mute: 'F10',
        toggle_fullscreen: 'F11'
    };

    function onElementEvent(el, type, handler) {
        if (el && typeof el.addEventListener === 'function') {
            el.addEventListener(type, handler);
        }
    }

    function formatCapLabel(cap) {
        if (cap === null || cap === undefined) {
            return 'Unlimited';
        }
        if (cap === 0) {
            return '0 FPS (Paused)';
        }
        return String(cap) + ' FPS';
    }

    function presetToFps(preset) {
        switch (preset) {
            case 'Fps0': return 0;
            case 'Fps5': return 5;
            case 'Fps15': return 15;
            case 'Fps30': return 30;
            case 'Fps60': return 60;
            case 'Fps120': return 120;
            case 'Fps144': return 144;
            case 'Fps240': return 240;
            case 'Unlimited': return null;
            default: return null;
        }
    }

    function recomputeEffectiveCap(st) {
        if (!st || !st.settings) return null;
        const isForeground = st.window_focused !== false && !st.window_minimized && st.window_visible !== false;
        const cap = isForeground
            ? presetToFps(st.settings.foreground_fps_cap)
            : presetToFps(st.settings.background_fps_cap);
        st.effective_fps_cap = cap;
        return cap;
    }

    function updateFpsHud() {
        const st = window.__BLOCKIDLE_CLIENT_STATE__;
        const visible = Boolean(st && st.settings && st.settings.fps_hud_visible);
        if (!hudElement) {
            ensureOverlayMounted();
        }
        if (!hudElement) {
            return;
        }
        hudElement.style.display = visible ? 'block' : 'none';
        if (!visible) {
            return;
        }
        const cap = getEffectiveCap();
        const fps = getLiveFps();
        hudElement.textContent = fps + ' FPS | Cap: ' + formatCapLabel(cap);
    }

    function syncSettingsOverlay() {
        const st = window.__BLOCKIDLE_CLIENT_STATE__;
        if (!settingsOverlayElement) {
            return;
        }
        const open = Boolean(st && st.settings_overlay_open);
        settingsOverlayElement.style.display = open ? 'flex' : 'none';
        settingsOverlayElement.style.pointerEvents = open ? 'auto' : 'none';
        if (!st || !st.settings) {
            return;
        }
        if (fgSelectElement) {
            fgSelectElement.value = st.settings.foreground_fps_cap || 'Unlimited';
        }
        if (bgSelectElement) {
            bgSelectElement.value = st.settings.background_fps_cap || 'Fps15';
        }
        if (toggleElements.fps_hud_visible) {
            toggleElements.fps_hud_visible.checked = Boolean(st.settings.fps_hud_visible);
        }
        if (toggleElements.keep_awake) {
            toggleElements.keep_awake.checked = Boolean(st.settings.keep_awake);
        }
        if (toggleElements.close_to_tray) {
            toggleElements.close_to_tray.checked = Boolean(st.settings.close_to_tray);
        }
        if (toggleElements.audio_muted) {
            toggleElements.audio_muted.checked = Boolean(st.settings.audio_muted);
        }
        if (toggleElements.fullscreen) {
            toggleElements.fullscreen.checked = Boolean(st.settings.fullscreen);
        }
        const kb = st.settings.keybindings || DEFAULT_KEYBINDINGS;
        for (let i = 0; i < KEYBIND_ACTIONS.length; i++) {
            const act = KEYBIND_ACTIONS[i].key;
            const btn = keybindButtons[act];
            if (btn) {
                btn.textContent = (recordingAction === act)
                    ? 'Press a key...'
                    : (kb[act] || DEFAULT_KEYBINDINGS[act]);
            }
        }
        if (keybindStatusElement) {
            keybindStatusElement.textContent = keybindStatusText;
        }
    }

    function ensureOverlayMounted() {
        if (hudElement && settingsOverlayElement) {
            return;
        }
        const rootTarget = document.body || document.documentElement;
        if (!rootTarget) {
            return;
        }
        let host = document.getElementById('blockidle-client-overlay-host');
        if (!host) {
            host = document.createElement('div');
            host.id = 'blockidle-client-overlay-host';
            host.style.position = 'fixed';
            host.style.top = '0';
            host.style.left = '0';
            host.style.width = '100%';
            host.style.height = '100%';
            host.style.pointerEvents = 'none';
            host.style.zIndex = '2147483647';
            rootTarget.appendChild(host);
        }
        const shadow = host.shadowRoot || host.attachShadow({ mode: 'open' });
        let styleEl = shadow.getElementById ? shadow.getElementById('blockidle-overlay-style') : null;
        if (!styleEl) {
            styleEl = document.createElement('style');
            styleEl.id = 'blockidle-overlay-style';
            styleEl.textContent =
                '#blockidle-fps-hud { position: fixed; top: 12px; left: 12px; padding: 6px 10px; ' +
                'background: rgba(15, 18, 24, 0.78); color: #e6edf3; font-family: monospace; ' +
                'font-size: 12px; font-weight: 600; border-radius: 6px; border: 1px solid rgba(255, 255, 255, 0.14); ' +
                'pointer-events: none; user-select: none; z-index: 2147483647; } ' +
                '#blockidle-settings-overlay { position: fixed; top: 0; left: 0; width: 100%; height: 100%; ' +
                'background: rgba(8, 10, 15, 0.72); display: none; align-items: center; justify-content: center; ' +
                'font-family: system-ui, -apple-system, sans-serif; color: #e6edf3; z-index: 2147483647; pointer-events: auto; } ' +
                '#blockidle-settings-panel { width: 460px; max-width: 92vw; max-height: 88vh; overflow-y: auto; ' +
                'background: #131722; border: 1px solid rgba(255, 255, 255, 0.16); border-radius: 12px; padding: 20px; ' +
                'box-shadow: 0 20px 50px rgba(0, 0, 0, 0.6); } ' +
                '.bi-row { display: flex; align-items: center; justify-content: space-between; padding: 8px 0; border-bottom: 1px solid rgba(255,255,255,0.06); } ' +
                '.bi-btn, select { background: #1f2637; color: #e6edf3; border: 1px solid rgba(255,255,255,0.2); border-radius: 6px; padding: 6px 12px; cursor: pointer; font-size: 13px; }';
            shadow.appendChild(styleEl);
        }
        let hud = shadow.getElementById ? shadow.getElementById('blockidle-fps-hud') : null;
        if (!hud) {
            hud = document.createElement('div');
            hud.id = 'blockidle-fps-hud';
            hud.style.pointerEvents = 'none';
            shadow.appendChild(hud);
        }
        hudElement = hud;

        let overlay = shadow.getElementById ? shadow.getElementById('blockidle-settings-overlay') : null;
        if (!overlay) {
            overlay = document.createElement('div');
            overlay.id = 'blockidle-settings-overlay';
            overlay.style.display = 'none';
            overlay.style.pointerEvents = 'none';

            const panel = document.createElement('div');
            panel.id = 'blockidle-settings-panel';

            const header = document.createElement('div');
            header.className = 'bi-row';
            const title = document.createElement('h2');
            title.textContent = 'Settings Overlay';
            const closeBtn = document.createElement('button');
            closeBtn.id = 'blockidle-close-overlay';
            closeBtn.className = 'bi-btn';
            closeBtn.textContent = 'Close (Esc)';
            onElementEvent(closeBtn, 'click', function() {
                const st = window.__BLOCKIDLE_CLIENT_STATE__;
                if (st) {
                    recordingAction = null;
                    st.settings_overlay_open = false;
                    syncSettingsOverlay();
                    sendIpc({ type: 'set_settings_overlay_open', open: false });
                }
            });
            header.appendChild(title);
            header.appendChild(closeBtn);
            panel.appendChild(header);

            function createFpsSelectRow(id, labelText, settingField, presets) {
                const row = document.createElement('div');
                row.className = 'bi-row';
                const label = document.createElement('label');
                label.textContent = labelText;
                const select = document.createElement('select');
                select.id = id;
                for (let i = 0; i < presets.length; i++) {
                    const opt = document.createElement('option');
                    opt.value = presets[i][0];
                    opt.textContent = presets[i][1];
                    select.appendChild(opt);
                }
                onElementEvent(select, 'change', function() {
                    const st = window.__BLOCKIDLE_CLIENT_STATE__;
                    if (st && st.settings) {
                        st.settings[settingField] = select.value;
                        recomputeEffectiveCap(st);
                        window.__blockidleApplyState(st);
                        sendIpc({ type: 'update_settings', settings: st.settings });
                    }
                });
                row.appendChild(label);
                row.appendChild(select);
                panel.appendChild(row);
                return select;
            }

            fgSelectElement = createFpsSelectRow(
                'blockidle-fg-fps-select',
                'Foreground FPS Cap',
                'foreground_fps_cap',
                [
                    ['Fps15', '15 FPS'],
                    ['Fps30', '30 FPS'],
                    ['Fps60', '60 FPS'],
                    ['Fps120', '120 FPS'],
                    ['Fps144', '144 FPS'],
                    ['Fps240', '240 FPS'],
                    ['Unlimited', 'Unlimited']
                ]
            );

            bgSelectElement = createFpsSelectRow(
                'blockidle-bg-fps-select',
                'Background FPS Cap',
                'background_fps_cap',
                [
                    ['Fps0', '0 FPS (Paused)'],
                    ['Fps5', '5 FPS'],
                    ['Fps15', '15 FPS'],
                    ['Fps30', '30 FPS'],
                    ['Fps60', '60 FPS']
                ]
            );

            // Toggles
            const toggleDefs = [
                { id: 'blockidle-toggle-fps-hud', field: 'fps_hud_visible', label: 'FPS HUD' },
                { id: 'blockidle-toggle-keep-awake', field: 'keep_awake', label: 'Keep Awake' },
                { id: 'blockidle-toggle-close-to-tray', field: 'close_to_tray', label: 'Close to Tray' },
                { id: 'blockidle-toggle-audio-mute', field: 'audio_muted', label: 'Audio Mute' },
                { id: 'blockidle-toggle-fullscreen', field: 'fullscreen', label: 'Fullscreen' }
            ];
            for (let i = 0; i < toggleDefs.length; i++) {
                const def = toggleDefs[i];
                const row = document.createElement('div');
                row.className = 'bi-row';
                const lbl = document.createElement('label');
                lbl.textContent = def.label;
                const input = document.createElement('input');
                input.type = 'checkbox';
                input.id = def.id;
                onElementEvent(input, 'change', function() {
                    const st = window.__BLOCKIDLE_CLIENT_STATE__;
                    if (st && st.settings) {
                        st.settings[def.field] = Boolean(input.checked);
                        window.__blockidleApplyState(st);
                        sendIpc({ type: 'update_settings', settings: st.settings });
                    }
                });
                row.appendChild(lbl);
                row.appendChild(input);
                panel.appendChild(row);
                toggleElements[def.field] = input;
            }

            // Keybindings section
            const kbStatus = document.createElement('div');
            kbStatus.id = 'blockidle-keybind-status';
            panel.appendChild(kbStatus);
            keybindStatusElement = kbStatus;

            for (let i = 0; i < KEYBIND_ACTIONS.length; i++) {
                const item = KEYBIND_ACTIONS[i];
                const row = document.createElement('div');
                row.className = 'bi-row';
                const lbl = document.createElement('span');
                lbl.textContent = item.label;
                const btn = document.createElement('button');
                btn.id = 'blockidle-keybind-' + item.key;
                btn.className = 'bi-btn';
                onElementEvent(btn, 'click', function() {
                    recordingAction = item.key;
                    keybindStatusText = 'Press a key or Esc to cancel';
                    syncSettingsOverlay();
                });
                row.appendChild(lbl);
                row.appendChild(btn);
                panel.appendChild(row);
                keybindButtons[item.key] = btn;
            }

            const restoreBtn = document.createElement('button');
            restoreBtn.id = 'blockidle-restore-defaults';
            restoreBtn.className = 'bi-btn';
            restoreBtn.textContent = 'Restore Defaults';
            onElementEvent(restoreBtn, 'click', function() {
                const st = window.__BLOCKIDLE_CLIENT_STATE__;
                recordingAction = null;
                keybindStatusText = '';
                if (st && st.settings) {
                    st.settings.keybindings = Object.assign({}, DEFAULT_KEYBINDINGS);
                }
                syncSettingsOverlay();
                sendIpc({ type: 'restore_default_keybindings' });
            });
            panel.appendChild(restoreBtn);

            overlay.appendChild(panel);
            shadow.appendChild(overlay);
        }
        settingsOverlayElement = overlay;
        updateFpsHud();
        syncSettingsOverlay();
    }

    function normalizeEventShortcut(event) {
        if (!event || typeof event.key !== 'string') {
            return null;
        }
        const rawKey = event.key;
        const lower = rawKey.toLowerCase();
        if (lower === 'control' || lower === 'ctrl' || lower === 'shift' || lower === 'alt' || lower === 'meta' || lower === 'os') {
            return { isModifierOnly: true };
        }
        if (lower === 'escape' || lower === 'esc') {
            return { isEscape: true };
        }

        let mainKey = null;
        if (/^f([1-9]|1[0-9]|2[0-4])$/i.test(rawKey)) {
            mainKey = rawKey.toUpperCase();
        } else if (rawKey.length === 1 && /^[a-z0-9]$/i.test(rawKey)) {
            mainKey = rawKey.toUpperCase();
        } else {
            const map = {
                'enter': 'Enter',
                'return': 'Enter',
                ' ': 'Space',
                'space': 'Space',
                'spacebar': 'Space',
                'tab': 'Tab',
                'backspace': 'Backspace',
                'delete': 'Delete',
                'del': 'Delete',
                'insert': 'Insert',
                'ins': 'Insert',
                'home': 'Home',
                'end': 'End',
                'pageup': 'PageUp',
                'pagedown': 'PageDown',
                'arrowup': 'ArrowUp',
                'arrowdown': 'ArrowDown',
                'arrowleft': 'ArrowLeft',
                'arrowright': 'ArrowRight',
                '`': 'Backquote',
                '-': 'Minus',
                '=': 'Equal',
                '[': 'BracketLeft',
                ']': 'BracketRight',
                '\\': 'Backslash',
                ';': 'Semicolon',
                '\'': 'Quote',
                ',': 'Comma',
                '.': 'Period',
                '/': 'Slash'
            };
            if (map[lower]) {
                mainKey = map[lower];
            }
        }

        if (!mainKey) {
            return null;
        }

        const parts = [];
        if (event.ctrlKey) parts.push('Ctrl');
        if (event.altKey) parts.push('Alt');
        if (event.shiftKey) parts.push('Shift');
        parts.push(mainKey);
        return { shortcut: parts.join('+') };
    }

    function consumeKeyboardEvent(event) {
        if (!event) return;
        if (typeof event.preventDefault === 'function') event.preventDefault();
        if (typeof event.stopImmediatePropagation === 'function') event.stopImmediatePropagation();
        if (typeof event.stopPropagation === 'function') event.stopPropagation();
    }

    function findActionForShortcut(kb, shortcut) {
        if (!kb || !shortcut) return null;
        const target = shortcut.toLowerCase();
        for (let i = 0; i < KEYBIND_ACTIONS.length; i++) {
            const act = KEYBIND_ACTIONS[i].key;
            const bound = kb[act];
            if (typeof bound === 'string' && bound.toLowerCase() === target) {
                return act;
            }
        }
        return null;
    }

    function handleKeyDownCapture(event) {
        if (!event || event.__blockidleHandled) {
            return;
        }
        event.__blockidleHandled = true;

        const st = window.__BLOCKIDLE_CLIENT_STATE__;
        const keyId = event.code || event.key || '';

        if (recordingAction !== null) {
            consumeKeyboardEvent(event);
            consumedKeys.add(keyId);
            if (event.repeat) {
                return;
            }
            const parsed = normalizeEventShortcut(event);
            if (!parsed) {
                return;
            }
            if (parsed.isEscape) {
                recordingAction = null;
                keybindStatusText = '';
                syncSettingsOverlay();
                return;
            }
            if (parsed.isModifierOnly) {
                return;
            }
            if (parsed.shortcut && st && st.settings && st.settings.keybindings) {
                const existing = findActionForShortcut(st.settings.keybindings, parsed.shortcut);
                if (existing && existing !== recordingAction) {
                    recordingAction = null;
                    keybindStatusText = 'Conflict: ' + parsed.shortcut + ' is already bound';
                    syncSettingsOverlay();
                    return;
                }
                const action = recordingAction;
                recordingAction = null;
                keybindStatusText = '';
                st.settings.keybindings[action] = parsed.shortcut;
                syncSettingsOverlay();
                sendIpc({ type: 'update_keybinding', action: action, shortcut: parsed.shortcut });
            }
            return;
        }

        if (st && st.settings_overlay_open) {
            consumeKeyboardEvent(event);
            consumedKeys.add(keyId);
            if (event.repeat) {
                return;
            }
            const parsed = normalizeEventShortcut(event);
            if (!parsed) {
                return;
            }
            if (parsed.isEscape) {
                st.settings_overlay_open = false;
                syncSettingsOverlay();
                sendIpc({ type: 'set_settings_overlay_open', open: false });
                return;
            }
            if (parsed.shortcut && st.settings && st.settings.keybindings) {
                const matched = findActionForShortcut(st.settings.keybindings, parsed.shortcut);
                if (matched === 'toggle_settings_overlay') {
                    st.settings_overlay_open = false;
                    syncSettingsOverlay();
                    sendIpc({ type: 'trigger_keybinding', shortcut: parsed.shortcut });
                } else if (matched) {
                    sendIpc({ type: 'trigger_keybinding', shortcut: parsed.shortcut });
                }
            }
            return;
        }

        if (!st || st.window_focused === false || st.window_visible === false || st.window_minimized) {
            return;
        }

        const parsed = normalizeEventShortcut(event);
        if (!parsed || !parsed.shortcut || !st.settings || !st.settings.keybindings) {
            return;
        }

        const matchedAction = findActionForShortcut(st.settings.keybindings, parsed.shortcut);
        if (!matchedAction) {
            return;
        }

        consumeKeyboardEvent(event);
        consumedKeys.add(keyId);
        if (event.repeat) {
            return;
        }
        ensureOverlayMounted();

        if (matchedAction === 'toggle_settings_overlay') {
            st.settings_overlay_open = !st.settings_overlay_open;
            syncSettingsOverlay();
        } else if (matchedAction === 'toggle_fps_hud') {
            st.settings.fps_hud_visible = !st.settings.fps_hud_visible;
            updateFpsHud();
            syncSettingsOverlay();
        } else if (matchedAction === 'toggle_audio_mute') {
            st.settings.audio_muted = !st.settings.audio_muted;
            syncAudioMuteState();
            syncSettingsOverlay();
        } else if (matchedAction === 'toggle_fullscreen') {
            st.settings.fullscreen = !st.settings.fullscreen;
            syncSettingsOverlay();
        }

        sendIpc({ type: 'trigger_keybinding', shortcut: parsed.shortcut });
    }

    function handleKeyUpCapture(event) {
        if (!event || event.__blockidleHandled) {
            return;
        }
        event.__blockidleHandled = true;
        const st = window.__BLOCKIDLE_CLIENT_STATE__;
        const keyId = event.code || event.key || '';
        if (recordingAction !== null || (st && st.settings_overlay_open) || consumedKeys.has(keyId)) {
            consumedKeys.delete(keyId);
            consumeKeyboardEvent(event);
        }
    }

    window.addEventListener('keydown', handleKeyDownCapture, true);
    document.addEventListener('keydown', handleKeyDownCapture, true);
    window.addEventListener('keyup', handleKeyUpCapture, true);
    document.addEventListener('keyup', handleKeyUpCapture, true);

    function recordDeliveredFrame(now) {
        deliveredFrameTimestamps.push(now);
        const cutoff = now - 1000;
        while (deliveredFrameTimestamps.length > 0 && deliveredFrameTimestamps[0] <= cutoff) {
            deliveredFrameTimestamps.shift();
        }
        liveFps = deliveredFrameTimestamps.length;
        updateFpsHud();
    }

    function getLiveFps() {
        const now = (window.performance && typeof window.performance.now === 'function')
            ? window.performance.now()
            : Date.now();
        const cutoff = now - 1000;
        while (deliveredFrameTimestamps.length > 0 && deliveredFrameTimestamps[0] <= cutoff) {
            deliveredFrameTimestamps.shift();
        }
        if (getEffectiveCap() === 0) {
            return 0;
        }
        liveFps = deliveredFrameTimestamps.length;
        return liveFps;
    }

    function clearScheduledPump() {
        if (scheduledNativeRafId !== null) {
            nativeCancelRaf(scheduledNativeRafId);
            scheduledNativeRafId = null;
        }
        if (scheduledTimerId !== null) {
            nativeClearTimeout(scheduledTimerId);
            scheduledTimerId = null;
        }
    }

    function flushCallbacks(timestamp) {
        const batch = pendingCallbacks.splice(0, pendingCallbacks.length);
        if (batch.length === 0) {
            return;
        }
        activeBatch = batch;
        recordDeliveredFrame(timestamp);
        for (let i = 0; i < batch.length; i++) {
            if (batch[i].cancelled) {
                continue;
            }
            try {
                batch[i].callback(timestamp);
            } catch (err) {
                nativeSetTimeout(function() { throw err; }, 0);
            }
        }
        activeBatch = null;
    }

    function schedulePump() {
        if (pendingCallbacks.length === 0) {
            return;
        }
        const cap = getEffectiveCap();
        if (cap === 0) {
            clearScheduledPump();
            return;
        }
        if (scheduledNativeRafId !== null || scheduledTimerId !== null) {
            return;
        }
        if (cap === null || cap === undefined) {
            scheduledNativeRafId = nativeRaf(function(ts) {
                scheduledNativeRafId = null;
                lastFrameTimestamp = ts;
                flushCallbacks(ts);
                if (pendingCallbacks.length > 0) {
                    schedulePump();
                }
            });
            return;
        }

        const interval = 1000 / cap;
        const now = (window.performance && typeof window.performance.now === 'function')
            ? window.performance.now()
            : Date.now();
        const remaining = (lastFrameTimestamp + interval) - now;

        if (remaining <= 0.5) {
            scheduledNativeRafId = nativeRaf(function(ts) {
                scheduledNativeRafId = null;
                const currentCap = getEffectiveCap();
                if (currentCap === 0) {
                    return;
                }
                if (currentCap !== null && currentCap !== undefined) {
                    const targetInterval = 1000 / currentCap;
                    if (ts - lastFrameTimestamp < targetInterval - 0.5) {
                        schedulePump();
                        return;
                    }
                    if (!Number.isFinite(lastFrameTimestamp) || (ts - lastFrameTimestamp) > targetInterval * 2) {
                        lastFrameTimestamp = ts;
                    } else {
                        lastFrameTimestamp = lastFrameTimestamp + targetInterval;
                    }
                } else {
                    lastFrameTimestamp = ts;
                }
                flushCallbacks(ts);
                if (pendingCallbacks.length > 0) {
                    schedulePump();
                }
            });
        } else {
            scheduledTimerId = nativeSetTimeout(function() {
                scheduledTimerId = null;
                schedulePump();
            }, Math.max(0, Math.floor(remaining)));
        }
    }

    window.requestAnimationFrame = function(callback) {
        if (typeof callback !== 'function') {
            throw new TypeError('requestAnimationFrame callback must be a function');
        }
        const id = nextRequestId++;
        pendingCallbacks.push({ id: id, callback: callback, cancelled: false });
        schedulePump();
        return id;
    };

    window.cancelAnimationFrame = function(id) {
        for (let i = 0; i < pendingCallbacks.length; i++) {
            if (pendingCallbacks[i].id === id) {
                pendingCallbacks[i].cancelled = true;
            }
        }
        pendingCallbacks = pendingCallbacks.filter(function(entry) {
            return !entry.cancelled;
        });
        if (activeBatch) {
            for (let i = 0; i < activeBatch.length; i++) {
                if (activeBatch[i].id === id) {
                    activeBatch[i].cancelled = true;
                }
            }
        }
        if (pendingCallbacks.length === 0) {
            clearScheduledPump();
        }
    };

    function sendIpc(message) {
        if (window.ipc && typeof window.ipc.postMessage === 'function') {
            window.ipc.postMessage(JSON.stringify(message));
        }
    }

    function isAudioMuted() {
        var st = window.__BLOCKIDLE_CLIENT_STATE__;
        return Boolean(st && st.settings && st.settings.audio_muted);
    }

    var trackedAudioContexts = [];
    var trackedMediaElements = new Set();

    function wrapAudioContextConstructor(ctorName) {
        var NativeAudioContext = window[ctorName];
        if (typeof NativeAudioContext !== 'function') {
            return;
        }
        function WrappedAudioContext() {
            var ctx = Reflect.construct(NativeAudioContext, arguments, new.target || WrappedAudioContext);
            var nativeResume = typeof ctx.resume === 'function' ? ctx.resume.bind(ctx) : null;
            var nativeSuspend = typeof ctx.suspend === 'function' ? ctx.suspend.bind(ctx) : null;
            ctx.__blockidleNativeResume = nativeResume;
            ctx.__blockidleNativeSuspend = nativeSuspend;
            if (nativeResume) {
                ctx.resume = function() {
                    if (isAudioMuted()) {
                        if (nativeSuspend) {
                            return nativeSuspend();
                        }
                        return Promise.resolve();
                    }
                    return nativeResume();
                };
            }
            trackedAudioContexts.push(ctx);
            if (isAudioMuted() && nativeSuspend) {
                try { nativeSuspend(); } catch (_) {}
            }
            return ctx;
        }
        WrappedAudioContext.prototype = NativeAudioContext.prototype;
        Object.setPrototypeOf(WrappedAudioContext, NativeAudioContext);
        window[ctorName] = WrappedAudioContext;
    }

    wrapAudioContextConstructor('AudioContext');
    wrapAudioContextConstructor('webkitAudioContext');

    if (typeof window.HTMLMediaElement !== 'undefined' && window.HTMLMediaElement.prototype) {
        var nativePlay = window.HTMLMediaElement.prototype.play;
        if (typeof nativePlay === 'function') {
            window.HTMLMediaElement.prototype.play = function() {
                trackedMediaElements.add(this);
                if (isAudioMuted()) {
                    this.muted = true;
                }
                return nativePlay.apply(this, arguments);
            };
        }
    }

    function syncAudioMuteState() {
        var muted = isAudioMuted();
        for (var i = 0; i < trackedAudioContexts.length; i++) {
            var ctx = trackedAudioContexts[i];
            if (!ctx || ctx.state === 'closed') {
                continue;
            }
            try {
                if (muted && typeof ctx.__blockidleNativeSuspend === 'function') {
                    ctx.__blockidleNativeSuspend();
                } else if (!muted && typeof ctx.__blockidleNativeResume === 'function') {
                    ctx.__blockidleNativeResume();
                }
            } catch (_) {}
        }
        if (typeof document !== 'undefined' && typeof document.querySelectorAll === 'function') {
            var domMedia = document.querySelectorAll('audio, video');
            for (var j = 0; j < domMedia.length; j++) {
                trackedMediaElements.add(domMedia[j]);
            }
        }
        trackedMediaElements.forEach(function(media) {
            if (media) {
                media.muted = muted;
            }
        });
    }

    window.__blockidleApplyState = function(nextState) {
        const prevCap = getEffectiveCap();
        window.__BLOCKIDLE_CLIENT_STATE__ = nextState;
        syncAudioMuteState();
        const nextCap = getEffectiveCap();
        if (nextCap === 0) {
            clearScheduledPump();
        } else if (prevCap === 0 && (nextCap === null || nextCap > 0)) {
            lastFrameTimestamp = -Infinity;
            clearScheduledPump();
            schedulePump();
        } else if (prevCap !== nextCap) {
            clearScheduledPump();
            schedulePump();
        }
        updateFpsHud();
        syncSettingsOverlay();
        window.dispatchEvent(new CustomEvent('blockidle:state', { detail: nextState }));
    };

    syncAudioMuteState();

    window.__BLOCKIDLE_CLIENT__ = {
        getState: function() { return window.__BLOCKIDLE_CLIENT_STATE__; },
        getLiveFps: getLiveFps,
        sendIpc: sendIpc,
        applyState: window.__blockidleApplyState
    };

    if (typeof window.setInterval === 'function') {
        window.setInterval(updateFpsHud, 250);
    }

    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', function() {
            syncAudioMuteState();
            ensureOverlayMounted();
            sendIpc({ type: 'client_ready' });
        }, { once: true });
    } else {
        ensureOverlayMounted();
        sendIpc({ type: 'client_ready' });
    }
})();
