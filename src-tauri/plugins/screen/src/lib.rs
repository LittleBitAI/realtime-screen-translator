//! The Kotlin side of the Android app (A1, A2, A6): `ScreenPlugin` in
//! `android/`. Rust calls it from the capture loop thread and the backend's
//! platform layer; every call blocks until Kotlin answers.

use serde::de::DeserializeOwned;
use serde_json::Value;
use tauri::plugin::{Builder, TauriPlugin};
use tauri::{Manager, Runtime};

/// The registered Kotlin plugin.
pub struct Screen<R: Runtime> {
    #[cfg(target_os = "android")]
    handle: tauri::plugin::PluginHandle<R>,
    #[cfg(not(target_os = "android"))]
    _r: std::marker::PhantomData<fn() -> R>,
}

impl<R: Runtime> Screen<R> {
    /// Run `command` of `ScreenPlugin` with `args` and read its answer.
    pub fn call<T: DeserializeOwned>(&self, command: &str, args: Value) -> Result<T, String> {
        #[cfg(target_os = "android")]
        return self.handle.run_mobile_plugin(command, args).map_err(|e| format!("{command}: {e}"));
        #[cfg(not(target_os = "android"))]
        {
            let _ = args;
            Err(format!("{command}: the screen plugin runs on Android only"))
        }
    }
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("rst-screen")
        .setup(|app, _api| {
            #[cfg(target_os = "android")]
            let screen = Screen { handle: _api.register_android_plugin("ai.littlebit.rst.screen", "ScreenPlugin")? };
            #[cfg(not(target_os = "android"))]
            let screen = Screen::<R> { _r: std::marker::PhantomData };
            app.manage(screen);
            Ok(())
        })
        .build()
}
