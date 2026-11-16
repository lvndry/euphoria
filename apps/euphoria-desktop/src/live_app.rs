// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Live as part of the app: the model that outlives the main window, the
//! always-on-top panel, and the menu bar item. Closing the main window
//! leaves all of this running when the user keeps Live running.

use euphoria_core::live::AudioCapture;
use gpui::prelude::*;
use std::path::PathBuf;

use gpui::{
    AnyWindowHandle, App, Bounds, Entity, Global, Pixels, WeakEntity, WindowBounds, WindowKind,
    WindowOptions, point, px, size,
};

use crate::app::EuphoriaApp;
use crate::live_model::{CaptureFactory, LiveModel};
use crate::menu_bar::{PanelAnchor, ScreenFrame};
use crate::settings::Settings;
use crate::views::live::{LiveView, PANEL_HEIGHT, PANEL_WIDTH};

const PANEL_MARGIN: f32 = 12.0;
const PANEL_TOP: f32 = 36.0;

struct LiveRuntime {
    model: Entity<LiveModel>,
    panel: Option<AnyWindowHandle>,
    menu_bar: bool,
    anchor: Option<PanelAnchor>,
    closed_at: Option<std::time::Instant>,
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
        anchor: None,
        closed_at: None,
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

fn screen_bounds(screen: ScreenFrame) -> Bounds<Pixels> {
    Bounds {
        origin: point(px(screen.left as f32), px(screen.top as f32)),
        size: size(px(screen.width as f32), px(screen.height as f32)),
    }
}

/// Where the panel's top left goes: under the item, kept on the item's own
/// screen, or near the top right of `display` when there is no item.
fn panel_origin(
    anchor: Option<PanelAnchor>,
    display: Option<Bounds<Pixels>>,
    width: Pixels,
) -> gpui::Point<Pixels> {
    if let Some(anchor) = anchor {
        let bounds = screen_bounds(anchor.screen);
        let left = (px(anchor.center_x as f32) - width / 2.0)
            .max(bounds.origin.x + px(PANEL_MARGIN))
            .min(bounds.origin.x + bounds.size.width - width - px(PANEL_MARGIN));
        return point(left, px(anchor.top as f32 + 4.0));
    }
    display.map_or(point(px(80.0), px(80.0)), |bounds| {
        point(
            bounds.origin.x + bounds.size.width - width - px(PANEL_MARGIN),
            bounds.origin.y + px(PANEL_TOP),
        )
    })
}

fn panel_options(anchor: Option<PanelAnchor>, cx: &App) -> (WindowOptions, (f32, f32)) {
    let width = px(PANEL_WIDTH);
    let height = px(PANEL_HEIGHT);
    let display = cx.primary_display().map(|display| display.bounds());
    let origin = panel_origin(anchor, display, width);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin,
            size: size(width, height),
        })),
        titlebar: None,
        kind: WindowKind::PopUp,
        is_resizable: false,
        is_minimizable: false,
        window_background: gpui::WindowBackgroundAppearance::Transparent,
        ..Default::default()
    };
    (options, (origin.x / px(1.0), origin.y / px(1.0)))
}

/// How soon after the panel closed itself a click on the menu bar item still
/// counts as the click that closed it.
const REOPEN_GUARD: std::time::Duration = std::time::Duration::from_millis(400);

/// Opens the Live panel and starts listening, or brings the panel forward
/// when it is open. It opens under the menu bar item when its place is
/// known.
pub(crate) fn open_panel(cx: &mut App) {
    open_panel_at(None, cx);
}

fn open_panel_at(anchor: Option<PanelAnchor>, cx: &mut App) {
    let model = runtime_model(cx);
    if let Some(anchor) = anchor {
        cx.global_mut::<LiveRuntime>().anchor = Some(anchor);
    }
    let anchor = cx.global::<LiveRuntime>().anchor;
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
    let (options, top_left) = panel_options(anchor, cx);
    let view_model = model.clone();
    match gpui::open_window(options, cx, |window, cx| {
        let view = cx.new(|cx| LiveView::new(view_model, cx));
        view.update(cx, |_, cx| {
            let was_active = std::cell::Cell::new(false);
            cx.observe_window_activation(window, move |_, window, cx| {
                if window.is_window_active() {
                    was_active.set(true);
                } else if was_active.get() {
                    cx.global_mut::<LiveRuntime>().closed_at = Some(std::time::Instant::now());
                    window.remove_window();
                }
            })
            .detach();
        });
        view
    }) {
        Ok((window, view)) => {
            view.update(cx, |view, _| view.top_left = Some(top_left));
            cx.global_mut::<LiveRuntime>().panel = Some(window);
            cx.activate(true);
            window
                .update(cx, |_, window, cx| {
                    let focus = view.read(cx).focus.clone();
                    focus.focus(window, cx);
                })
                .ok();
        }
        Err(error) => eprintln!("error: cannot open Live: {error:#}"),
    }
}

/// Hides the Live panel when it is open, and shows it otherwise.
pub(crate) fn toggle_panel(anchor: Option<PanelAnchor>, cx: &mut App) {
    let open_panel_handle = cx
        .try_global::<LiveRuntime>()
        .and_then(|runtime| runtime.panel)
        .filter(|panel| cx.windows().contains(panel));
    match open_panel_handle {
        Some(panel) => {
            panel.update(cx, |_, window, _| window.remove_window()).ok();
            cx.global_mut::<LiveRuntime>().panel = None;
        }
        None => {
            let just_closed = cx
                .try_global::<LiveRuntime>()
                .and_then(|runtime| runtime.closed_at)
                .is_some_and(|at| at.elapsed() < REOPEN_GUARD);
            if !just_closed {
                open_panel_at(anchor, cx);
            }
        }
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

/// Turns "Keep Live running when the window is closed" on or off.
pub(crate) fn toggle_keep_running(cx: &mut App) {
    let model = runtime_model(cx);
    model.update(cx, |model, cx| {
        let keep = !model.settings.keep_running_when_closed;
        model.set_keep_running(keep, cx);
    });
}

/// Turns opening Euphoria at login, with Live in the menu bar, on or off.
pub(crate) fn toggle_open_at_login(cx: &mut App) {
    let model = runtime_model(cx);
    model.update(cx, |model, cx| model.toggle_open_at_login(cx));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anchor_on(screen: ScreenFrame, center_x: i32) -> PanelAnchor {
        PanelAnchor {
            center_x,
            top: screen.top + 24,
            screen,
        }
    }

    #[test]
    fn a_menu_item_on_another_display_opens_the_panel_on_that_display() {
        let secondary = ScreenFrame {
            left: 1440,
            top: -100,
            width: 1920,
            height: 1080,
        };
        let origin = panel_origin(Some(anchor_on(secondary, 2200)), None, px(PANEL_WIDTH));

        assert_eq!(origin.x, px(2200.0 - PANEL_WIDTH / 2.0));
        assert_eq!(origin.y, px(-100.0 + 24.0 + 4.0));
    }

    #[test]
    fn an_item_near_the_edge_keeps_the_panel_on_its_own_display() {
        let secondary = ScreenFrame {
            left: 1440,
            top: 0,
            width: 1920,
            height: 1080,
        };
        let near_right = panel_origin(Some(anchor_on(secondary, 3350)), None, px(PANEL_WIDTH));
        let near_left = panel_origin(Some(anchor_on(secondary, 1450)), None, px(PANEL_WIDTH));

        assert_eq!(
            near_right.x,
            px(1440.0 + 1920.0 - PANEL_WIDTH - PANEL_MARGIN)
        );
        assert_eq!(near_left.x, px(1440.0 + PANEL_MARGIN));
    }

    #[test]
    fn without_an_item_the_panel_opens_near_the_top_right_of_the_display() {
        let primary = Bounds {
            origin: point(px(0.0), px(0.0)),
            size: size(px(1440.0), px(900.0)),
        };
        let origin = panel_origin(None, Some(primary), px(PANEL_WIDTH));

        assert_eq!(origin.x, px(1440.0 - PANEL_WIDTH - PANEL_MARGIN));
        assert_eq!(origin.y, px(PANEL_TOP));
    }
}
