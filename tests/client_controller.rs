use blockidle_app::{
    BackgroundFpsCap, ClientController, ClientSettings, ForegroundFpsCap, Keybindings,
    NavigationDecision, PlatformEffect,
};
use tempfile::tempdir;

#[test]
fn initializes_default_client_settings_and_game_view_url_when_config_missing() {
    let dir = tempdir().expect("tempdir");
    let controller = ClientController::load_or_default(dir.path());

    assert_eq!(controller.game_view_url(), "https://blockidle.io/play");

    let settings = controller.settings();
    assert_eq!(settings.foreground_fps_cap, ForegroundFpsCap::Unlimited);
    assert_eq!(settings.background_fps_cap, BackgroundFpsCap::Fps15);
    assert!(settings.keep_awake);
    assert!(!settings.close_to_tray);
    assert!(!settings.audio_muted);
    assert!(!settings.fullscreen);
    assert!(!settings.fps_hud_visible);
    assert_eq!(settings.window_width, 1280);
    assert_eq!(settings.window_height, 720);
    assert_eq!(
        settings.keybindings,
        Keybindings {
            toggle_settings_overlay: "F2".to_string(),
            toggle_fps_hud: "F3".to_string(),
            reload_game_view: "F5".to_string(),
            hide_to_tray: "F9".to_string(),
            toggle_audio_mute: "F10".to_string(),
            toggle_fullscreen: "F11".to_string(),
        }
    );
}

#[test]
fn persists_client_settings_to_disk_and_restores_on_reload() {
    let dir = tempdir().expect("tempdir");
    let mut controller = ClientController::load_or_default(dir.path());

    let updated = ClientSettings {
        foreground_fps_cap: ForegroundFpsCap::Fps144,
        background_fps_cap: BackgroundFpsCap::Fps5,
        fps_hud_visible: true,
        keep_awake: false,
        close_to_tray: true,
        audio_muted: true,
        fullscreen: true,
        window_width: 1920,
        window_height: 1080,
        keybindings: Keybindings {
            toggle_settings_overlay: "Ctrl+F2".to_string(),
            toggle_fps_hud: "F4".to_string(),
            reload_game_view: "Ctrl+R".to_string(),
            hide_to_tray: "Alt+F9".to_string(),
            toggle_audio_mute: "F8".to_string(),
            toggle_fullscreen: "Alt+Enter".to_string(),
        },
    };

    controller
        .update_settings(updated.clone())
        .expect("save settings");

    let reloaded = ClientController::load_or_default(dir.path());
    assert_eq!(reloaded.settings(), &updated);
}

#[test]
fn falls_back_to_defaults_when_settings_file_is_corrupted_and_provides_persistent_data_dir() {
    let config_dir = tempdir().expect("config tempdir");
    let data_dir = tempdir().expect("data tempdir");
    std::fs::write(config_dir.path().join("settings.json"), "{ corrupted json !!!")
        .expect("write corrupted file");

    let controller = ClientController::with_paths(config_dir.path(), data_dir.path());
    assert_eq!(controller.settings(), &ClientSettings::default());
    assert_eq!(controller.data_dir(), data_dir.path());

    let os_paths = blockidle_app::AppPaths::resolve().expect("resolve standard OS paths");
    assert!(os_paths.config_dir.ends_with("blockidle") || os_paths.config_dir.ends_with("Blockidle") || !os_paths.config_dir.as_os_str().is_empty());
    assert!(!os_paths.data_dir.as_os_str().is_empty());
}

#[test]
fn classifies_navigation_policy_for_game_view_stripe_turnstile_and_external_links() {
    let dir = tempdir().expect("tempdir");
    let mut controller = ClientController::load_or_default(dir.path());

    for allowed_url in [
        "https://blockidle.io/play",
        "https://blockidle.io/play?pago=pi_12345",
        "https://sub.blockidle.io/auth",
        "https://challenges.cloudflare.com/turnstile/v0/api.js",
        "https://checkout.stripe.com/c/pay/cs_live_123",
        "https://stripe.com/docs",
    ] {
        let (decision, effects) = controller.handle_navigation_request(allowed_url);
        assert_eq!(
            decision,
            NavigationDecision::AllowInGameView,
            "expected {allowed_url} to stay inside Game View"
        );
        assert!(effects.is_empty());
    }

    let discord_url = "https://discord.gg/blockidle";
    let (decision, effects) = controller.handle_navigation_request(discord_url);
    assert_eq!(
        decision,
        NavigationDecision::OpenExternalBrowser(discord_url.to_string())
    );
    assert_eq!(
        effects,
        vec![PlatformEffect::OpenExternalBrowser(discord_url.to_string())]
    );

    let (popup_decision, popup_effects) = controller.handle_new_window_request(discord_url);
    assert_eq!(
        popup_decision,
        NavigationDecision::OpenExternalBrowser(discord_url.to_string())
    );
    assert_eq!(
        popup_effects,
        vec![PlatformEffect::OpenExternalBrowser(discord_url.to_string())]
    );
}

#[test]
fn generates_initialization_script_and_processes_ipc_messages_from_game_view() {
    let dir = tempdir().expect("tempdir");
    let mut controller = ClientController::load_or_default(dir.path());

    let init_script = controller.initialization_script();
    assert!(init_script.contains("window.ipc.postMessage"));
    assert!(init_script.contains("__BLOCKIDLE_CLIENT_STATE__"));
    assert!(init_script.contains("__blockidleApplyState"));
    assert!(init_script.contains("\"keep_awake\":true"));

    // Simulate Game View sending `client_ready` over `window.ipc.postMessage`
    let ready_effects = controller.handle_ipc_message(r#"{"type":"client_ready"}"#);
    assert_eq!(ready_effects.len(), 1);
    match &ready_effects[0] {
        PlatformEffect::SyncWebViewState(script) => {
            assert!(script.contains("__blockidleApplyState"));
            assert!(script.contains("\"keep_awake\":true"));
        }
        other => panic!("expected SyncWebViewState, got {other:?}"),
    }

    // Simulate Game View updating settings via IPC and verify persistence + host effects
    let mut new_settings = controller.settings().clone();
    new_settings.foreground_fps_cap = ForegroundFpsCap::Fps60;
    new_settings.fullscreen = true;
    let ipc_payload = serde_json::json!({
        "type": "update_settings",
        "settings": new_settings,
    })
    .to_string();

    let effects = controller.handle_ipc_message(&ipc_payload);
    assert_eq!(controller.settings().foreground_fps_cap, ForegroundFpsCap::Fps60);
    assert!(controller.settings().fullscreen);
    assert!(effects.contains(&PlatformEffect::SetFullscreen(true)));
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, PlatformEffect::SyncWebViewState(_)))
    );

    let reloaded = ClientController::load_or_default(dir.path());
    assert_eq!(reloaded.settings().foreground_fps_cap, ForegroundFpsCap::Fps60);
    assert!(reloaded.settings().fullscreen);
}

#[test]
fn persists_window_dimensions_on_resize_and_handles_window_lifecycle_events() {
    let dir = tempdir().expect("tempdir");
    let mut controller = ClientController::load_or_default(dir.path());

    controller.handle_window_resized(1600, 900);
    let reloaded = ClientController::load_or_default(dir.path());
    assert_eq!(reloaded.settings().window_width, 1600);
    assert_eq!(reloaded.settings().window_height, 900);

    let close_effects = controller.handle_close_requested();
    assert_eq!(close_effects, vec![PlatformEffect::ExitApp]);
}





