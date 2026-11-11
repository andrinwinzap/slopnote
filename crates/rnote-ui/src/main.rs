#![warn(missing_debug_implementations)]
#![allow(clippy::single_match)]
// Turns off console window on Windows, but not when building with dev profile.
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

// Modules
pub(crate) mod app;
pub(crate) mod appmenu;
pub(crate) mod appwindow;
pub(crate) mod axisticks;
pub(crate) mod bookmarkspanel;
pub(crate) mod boxed;
pub(crate) mod canvas;
pub(crate) mod canvasmenu;
pub(crate) mod canvaswrapper;
pub(crate) mod colorpicker;
pub(crate) mod config;
pub(crate) mod contextmenu;
pub(crate) mod dialogs;
pub(crate) mod env;
pub(crate) mod filetype;
pub(crate) mod globals;
pub(crate) mod groupediconpicker;
pub(crate) mod iconpicker;
pub(crate) mod mainheader;
pub(crate) mod overlays;
pub(crate) mod penpicker;
pub(crate) mod penssidebar;
pub(crate) mod settingspanel;
pub(crate) mod sidebar;
pub(crate) mod strokecontentpaintable;
pub(crate) mod strokecontentpreview;
pub(crate) mod strokewidthpicker;
pub(crate) mod unitentry;
pub(crate) mod utils;
pub(crate) mod workspacebrowser;

// Re-exports
pub(crate) use app::RnApp;
pub(crate) use appmenu::RnAppMenu;
pub(crate) use appwindow::RnAppWindow;
pub(crate) use bookmarkspanel::RnBookmarksPanel;
pub(crate) use canvas::RnCanvas;
pub(crate) use canvasmenu::RnCanvasMenu;
pub(crate) use canvaswrapper::RnCanvasWrapper;
pub(crate) use colorpicker::RnColorPicker;
pub(crate) use contextmenu::RnContextMenu;
pub(crate) use filetype::FileType;
pub(crate) use groupediconpicker::RnGroupedIconPicker;
pub(crate) use iconpicker::RnIconPicker;
pub(crate) use mainheader::RnMainHeader;
pub(crate) use overlays::RnOverlays;
pub(crate) use penpicker::RnPenPicker;
pub(crate) use penssidebar::RnPensSideBar;
pub(crate) use settingspanel::RnSettingsPanel;
pub(crate) use sidebar::RnSidebar;
pub(crate) use strokecontentpaintable::StrokeContentPaintable;
pub(crate) use strokecontentpreview::RnStrokeContentPreview;
pub(crate) use strokewidthpicker::RnStrokeWidthPicker;
pub(crate) use unitentry::RnUnitEntry;
pub(crate) use workspacebrowser::RnWorkspaceBrowser;

// Renames
extern crate parry2d_f64 as p2d;

// Imports
use anyhow::Context;
use gtk4::{gio, glib, prelude::*};
use tracing::debug;

fn main() -> glib::ExitCode {
    if let Err(e) = setup_tracing() {
        eprintln!("failed to setup tracing, Err: {e:?}");
    }
    if let Err(e) = env::setup_env() {
        eprintln!("failed to setup env, Err: {e:?}");
    }
    if let Err(e) = setup_i18n() {
        eprintln!("failed to setup i18n, Err: {e:?}");
    }
    if let Err(e) = setup_gresources() {
        eprintln!("failed to setup gresources, Err: {e:?}");
    }

    // Without a handler, a panic in a task spawned on the thread pool (importing a damaged file,
    // rendering a stroke) aborts the whole application with every open document. With it only that
    // task is lost: its result channel closes, which the code waiting for it reports as an error.
    if let Err(e) = rayon::ThreadPoolBuilder::new()
        .panic_handler(|panic| {
            let message = panic
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown".to_string());
            tracing::error!("A background task panicked: {message}");
        })
        .build_global()
    {
        eprintln!("failed to setup the thread pool, Err: {e:?}");
    }

    let app = RnApp::new();
    app.run()
}

fn setup_tracing() -> anyhow::Result<()> {
    let timer = tracing_subscriber::fmt::time::Uptime::default();

    tracing_subscriber::fmt()
        .compact()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_timer(timer)
        .try_init()
        .map_err(|e| anyhow::anyhow!(e))?;
    debug!(".. tracing subscriber initialized.");
    Ok(())
}

fn setup_i18n() -> anyhow::Result<()> {
    let locale_dir = env::locale_dir()?;

    // # Safety
    //
    // setlocale() is called as early as possible, before starting threads or installing UNIX signals.
    unsafe { gettextrs::setlocale(gettextrs::LocaleCategory::LcAll, "") };
    gettextrs::bindtextdomain(config::GETTEXT_PACKAGE, locale_dir)?;
    gettextrs::bind_textdomain_codeset(config::GETTEXT_PACKAGE, "UTF-8")?;
    gettextrs::textdomain(config::GETTEXT_PACKAGE)?;
    Ok(())
}

fn setup_gresources() -> anyhow::Result<()> {
    gio::resources_register_include!("compiled.gresource")
        .context("Failed to register and include compiled gresource.")
}
