// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Live as part of the app: the model that outlives the main window, the
//! always-on-top panel, and the menu bar item. Closing the main window
//! leaves all of this running when the user keeps Live running.

use euphoria_core::live::AudioCapture;
use gpui::prelude::*;
use std::path::PathBuf;

use gpui::{
    AnyWindowHandle, App, Bounds, Entity, Global, TitlebarOptions, WeakEntity, WindowBounds,
    WindowKind, WindowOptions, point, px, size,
};

use crate::app::EuphoriaApp;
use crate::live_model::{CaptureFactory, LiveModel};
use crate::settings::Settings;
use crate::views::live::{LiveView, PANEL_HEIGHT, PANEL_WIDTH};

const PANEL_TITLE: &str = "Euphoria Live";
const PANEL_MARGIN: f32 = 12.0;
const PANEL_TOP: f32 = 36.0;

struct LiveRuntime {
    model: Entity<LiveModel>,
    panel: Option<AnyWindowHandle>,
    menu_bar: bool,
}

impl Global for LiveRuntime {}

/// The capture of this system: process taps on macOS.
pub(crate) fn system_capture_factory() -> CaptureFactory {
    Box::new(|| -> Box<dyn AudioCapture> {
        #[cfg(target_os = "macos")]
        {
            Box::new(euphoria_core::live::ProcessTapCapture::new())
        }
        #[cfg(not(target_os = "macos"))]
        {
            Box::new(UnsupportedCapture)
        }
    })
}

#[cfg(not(target_os = "macos"))]
struct UnsupportedCapture;

#[cfg(not(target_os = "macos"))]
impl AudioCapture for UnsupportedCapture {
    fn sources(&self) -> Vec<euphoria_core::live::ListenSource> {
        Vec::new()
    }

    fn start(
        &mut self,
        _source: &euphoria_core::live::ListenSource,
        _sink: euphoria_core::live::AudioSink,
    ) -> Result<std::num::NonZeroU32, euphoria_core::live::CaptureError> {
        Err(euphoria_core::live::CaptureError::Unsupported(
            "Live listens to the audio of the Mac it runs on; Linux is not supported yet.".into(),
        ))
    }

    fn stop(&mut self) {}
}

/// The model Live runs on, made the first time it is needed, with the menu
/// bar item it keeps up to date.
fn runtime_model(cx: &mut App) -> Entity<LiveModel> {
    if let Some(runtime) = cx.try_global::<LiveRuntime>() {
        return runtime.model.clone();
    }
    let model = cx.new(|cx| LiveModel::new(system_capture_factory(), cx));
    cx.set_global(LiveRuntime {
        model: model.clone(),
        panel: None,
        menu_bar: false,
    });
    install_menu_bar(&model, cx);
    model
}

fn install_menu_bar(model: &Entity<LiveModel>, cx: &mut App) {
    #[cfg(target_os = "macos")]
    if let Some((item, commands)) = crate::menu_bar_macos::connect() {
        crate::menu_bar::install(model, Box::new(item), commands, cx);
        cx.global_mut::<LiveRuntime>().menu_bar = true;
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (model, cx);
}

fn panel_options(cx: &App) -> WindowOptions {
    let width = px(PANEL_WIDTH);
    let height = px(PANEL_HEIGHT);
    let origin = cx
        .primary_display()
        .map_or(point(px(80.0), px(80.0)), |display| {
            let bounds = display.bounds();
            point(
                bounds.origin.x + bounds.size.width - width - px(PANEL_MARGIN),
                bounds.origin.y + px(PANEL_TOP),
            )
        });
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin,
            size: size(width, height),
        })),
        titlebar: Some(TitlebarOptions {
            title: Some(PANEL_TITLE.into()),
            appears_transparent: false,
            traffic_light_position: None,
        }),
        kind: WindowKind::Floating,
        is_resizable: false,
        is_minimizable: false,
        window_min_size: Some(size(width, height)),
        ..Default::default()
    }
}

/// Opens the Live panel and starts listening, or brings the panel forward
/// when it is open.
pub(crate) fn open_panel(cx: &mut App) {
    let model = runtime_model(cx);
    if let Some(panel) = cx.global::<LiveRuntime>().panel {
        if panel
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
        {
            cx.activate(true);
            return;
        }
        cx.global_mut::<LiveRuntime>().panel = None;
    }
    model.update(cx, |model, cx| {
        model.reload_inputs(Settings::load_or_default(), cx);
        if model.needs_first_start() {
            model.start(cx);
        }
    });
    let options = panel_options(cx);
    let view_model = model.clone();
    match gpui::open_window(options, cx, |_, cx| {
        cx.new(|cx| LiveView::new(view_model, cx))
    }) {
        Ok((window, _)) => {
            cx.global_mut::<LiveRuntime>().panel = Some(window);
            cx.activate(true);
        }
        Err(error) => eprintln!("error: cannot open Live: {error:#}"),
    }
}

/// Hides the Live panel when it is open, and shows it otherwise.
pub(crate) fn toggle_panel(cx: &mut App) {
    let open_panel_handle = cx
        .try_global::<LiveRuntime>()
        .and_then(|runtime| runtime.panel)
        .filter(|panel| cx.windows().contains(panel));
    match open_panel_handle {
        Some(panel) => {
            panel.update(cx, |_, window, _| window.remove_window()).ok();
            cx.global_mut::<LiveRuntime>().panel = None;
        }
        None => open_panel(cx),
    }
}

/// True when closing the last window should leave the app running for Live:
/// Live was started and the user keeps it running.
pub(crate) fn keeps_running(cx: &App) -> bool {
    cx.try_global::<LiveRuntime>()
        .is_some_and(|runtime| runtime.menu_bar && runtime.model.read(cx).settings.keeps_running())
}

/// Pauses or resumes listening, starting the panel's model first if Live was
/// not used yet.
pub(crate) fn toggle_listening(cx: &mut App) {
    let model = runtime_model(cx);
    model.update(cx, |model, cx| model.toggle_listening(cx));
}

/// The view of the library window, while it is open.
struct MainView(WeakEntity<EuphoriaApp>);

impl Global for MainView {}

/// How the library window is brought up. Nothing sets it but a test.
pub(crate) struct MainWindowOpener(pub(crate) std::rc::Rc<dyn Fn(&mut App)>);

impl Global for MainWindowOpener {}

pub(crate) fn remember_main_view(view: &Entity<EuphoriaApp>, cx: &mut App) {
    cx.set_global(MainView(view.downgrade()));
}

/// Brings up the library window, opening it when it was closed, and shows
/// the track at `path` in it, selected and in the inspector, without playing.
pub(crate) fn show_in_euphoria(path: PathBuf, cx: &mut App) {
    match cx
        .try_global::<MainWindowOpener>()
        .map(|opener| opener.0.clone())
    {
        Some(open) => open(cx),
        None => crate::open_euphoria(cx),
    }
    let view = cx
        .try_global::<MainView>()
        .and_then(|main| main.0.upgrade());
    if let Some(view) = view {
        view.update(cx, |app, cx| app.show_track_at_path(path, cx));
    }
}
