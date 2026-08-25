use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    App, AppHandle, Emitter, Listener, Manager, WindowEvent,
};

const MAIN_WINDOW_LABEL: &str = "main";
const OPEN_MENU_ID: &str = "tray-open";
const SETTINGS_MENU_ID: &str = "tray-settings";
const CHECK_UPDATE_MENU_ID: &str = "tray-check-update";
const QUIT_MENU_ID: &str = "tray-quit";
const TRAY_ACTION_EVENT: &str = "skillmate:tray-action";
const TRAY_LANGUAGE_EVENT: &str = "skillmate:tray-language";

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn dispatch_tray_action(app: &AppHandle, action: &str) {
    show_main_window(app);
    if let Err(error) = app.emit(TRAY_ACTION_EVENT, action) {
        eprintln!("发送托盘操作失败: {error}");
    }
}

pub fn setup(app: &mut App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, OPEN_MENU_ID, "打开 SkillMate", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, SETTINGS_MENU_ID, "设置…", true, None::<&str>)?;
    let check_update =
        MenuItem::with_id(app, CHECK_UPDATE_MENU_ID, "检查更新…", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, QUIT_MENU_ID, "退出 SkillMate", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &settings, &check_update, &separator, &quit])?;

    let localized_open = open.clone();
    let localized_settings = settings.clone();
    let localized_check_update = check_update.clone();
    let localized_quit = quit.clone();
    app.listen(TRAY_LANGUAGE_EVENT, move |event| {
        let language = serde_json::from_str::<String>(event.payload()).unwrap_or_default();
        let (open_text, settings_text, check_update_text, quit_text) = if language == "en" {
            (
                "Open SkillMate",
                "Settings…",
                "Check for Updates…",
                "Quit SkillMate",
            )
        } else {
            ("打开 SkillMate", "设置…", "检查更新…", "退出 SkillMate")
        };
        let _ = localized_open.set_text(open_text);
        let _ = localized_settings.set_text(settings_text);
        let _ = localized_check_update.set_text(check_update_text);
        let _ = localized_quit.set_text(quit_text);
    });

    let mut tray = TrayIconBuilder::with_id("skillmate-tray")
        .menu(&menu)
        .tooltip("SkillMate")
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            OPEN_MENU_ID => show_main_window(app),
            SETTINGS_MENU_ID => dispatch_tray_action(app, "settings"),
            CHECK_UPDATE_MENU_ID => dispatch_tray_action(app, "check-update"),
            QUIT_MENU_ID => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } | TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                }
            ) {
                show_main_window(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;

    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let window_to_hide = window.clone();
        window.on_window_event(move |event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window_to_hide.hide();
            }
        });
    }

    Ok(())
}
