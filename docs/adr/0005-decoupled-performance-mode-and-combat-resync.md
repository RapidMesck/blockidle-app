# Decoupled Performance Mode and Synthetic Combat Resynchronization

## Context

In `blockidle.io/play`, the native web client's built-in "Gráficos leves" (`bi-graficos = 'leve'`) couples a 30 FPS throttle with sun shadow removal. For desktop players, hardcoding 30 FPS degrades responsiveness on 60–240 Hz monitors even when only GPU load reduction is desired. Furthermore:
1. In high-resolution or multi-monitor setups, WebGL canvas rendering at native 1080p/1440p dominates GPU fill rate and power consumption.
2. Turning off `shadowMap.autoUpdate` in Three.js leaves existing shadow maps frozen on screen rather than disappearing, confusing players.
3. In idle combat sessions, backgrounding the client shell with a `0 FPS` (paused) cap caused the 3D scene and combat updates to halt. Upon returning, because Keep Awake permanently suppresses native `visibilitychange` events, the web game's network layer (`rede.js`) never ran `resync()`, leaving active mob IDs desynchronized in `C.mobs` and preventing hit mark floaters (`hud.textoDano`) and mob health bars from updating.

## Decision

We chose to decouple performance mode optimizations from frame-rate caps and implement synthetic combat resynchronization:

1. **Decoupled Performance Mode (`performance_mode`):**
   - Independent of the user's chosen Foreground and Background FPS caps.
   - When active, Three.js shadow maps are explicitly disabled (`renderer.shadowMap.enabled = false` and `autoUpdate = false`), and scene lights are traversed to set `castShadow = false`, eliminating shadow pass draw calls immediately.
   - The WebGL render buffer is downscaled to `0.75x` on standard (1x) displays (and `1.0x` on ≥2x Retina/HiDPI screens), reducing pixel fill rate by ~44% while setting `imageRendering: pixelated` on `<canvas id="cena">` to maintain crisp, authentic Minecraft pixel aesthetics.
   - Cosmetic cube particles (`window.__demo.particulas.criar`) are throttled by 80%.
   - Interceptors on `window.__renderizador` and `window.__demo` ensure these optimizations apply immediately upon Three.js instantiation even if the client shell initialized before the game loaded.

2. **Sensible Background Idle Cap & Synthetic Resync (`__blockidleResume`):**
   - Set the default `BackgroundFpsCap` to `15 FPS`, maintaining continuous combat updates and hit marks while reducing background GPU usage by ~90%.
   - When unpausing from `0 FPS` (user-selected paused mode), the Client Shell dispatches a synthetic `visibilitychange` event marked with `__blockidleResume = true`.
   - The Keep Awake suppression handler permits this synthetic resume event to pass through, allowing `rede.js` to run `resync()` and reconstruct `C.mobs` and HUD health bars without dropping the session.

3. **Global Reload Game View (`#blockidle-reload-game`):**
   - Added a dedicated *"Recarregar Jogo (F5)"* button inside the Settings Overlay that closes the overlay and triggers `PlatformEffect::ReloadGameView`, destroying and rebuilding the WebGL context and session state from scratch on demand.
