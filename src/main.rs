use app::{
    screenshot::ScreenshotApp, selection::SelectionApp, AppState, OldWaylandApp, OldWaylandContext,
    WaylandAppManager,
};
use clap::Parser;
use image::{ImageBuffer, Rgb};
use iter_tools::Itertools;
use points::{Point, Rectangle};
use wayland_client::Connection;

mod app;
mod dbg_time;
mod outputs;
mod points;

#[derive(Parser)]
struct Args {
    /// File to save screenshot
    #[arg(long, short, default_value = "image.png")]
    output: String,

    /// Do not use region selector
    #[arg(long, short)]
    fullscreen: bool,
}

enum ScreenshotResult {
    Selection {
        image: Box<[u8]>,
        rect: Rectangle,
        width: u32,
    },
    Canceled,
}

fn make_screenshot(args: &Args) -> Result<ScreenshotResult, app::Error> {
    let conn = Connection::connect_to_env().map_err(app::Error::Connect)?;
    // Initialize outputs
    let mut mgr = WaylandAppManager::initialize(&conn)?;

    // Make screenshot
    mgr.initialize_partial()?;
    mgr.next_app()?;
    mgr.dispatch_until_done()?;

    if args.fullscreen {
        let AppState::SelectionApp(SelectionApp { image, .. }) = mgr.app.state else {
            unreachable!("next app after base should be screenshot")
        };
        let ctx = mgr
            .app
            .ctx
            .partial()
            .expect("partial context should be initialized here");
        let (width, height) = (ctx.logical_size.x, ctx.logical_size.y);

        Ok(ScreenshotResult::Selection {
            image,
            width,
            rect: Rectangle::new(Point::new(0, 0), width, height),
        })
    } else {
        // Make selection
        mgr.initialize_full()?;
        mgr.next_app()?;
        mgr.dispatch_until_done()?;

        let (rect, image) = match mgr.app.state {
            AppState::SelectionApp(app) => (app.selected_region(), app.image),
            _ => unreachable!("next app after screenshot should be selection"),
        };

        let Some(rect) = rect else {
            return Ok(ScreenshotResult::Canceled);
        };

        let width = mgr
            .app
            .ctx
            .partial()
            .expect("partial context should be initialized here")
            .logical_size
            .x;

        Ok(ScreenshotResult::Selection { image, rect, width })
    }
}

fn main() {
    let args = Args::parse();

    let (image, rect, width) = match make_screenshot(&args) {
        Ok(ScreenshotResult::Selection { image, rect, width }) => (image, rect, width),
        Ok(ScreenshotResult::Canceled) => {
            eprintln!("selection canceled");
            std::process::exit(1);
        }

        Err(app::Error::Connect(c)) => {
            eprintln!("unable to connect to wayland server: {c}");
            std::process::exit(1);
        }
        Err(app::Error::Shm(e)) => {
            eprintln!("failed to initialize wl_shm: {e}");
            std::process::exit(1);
        }
        Err(app::Error::Zwlr(e)) => {
            eprintln!("failed to initialize zwlr_screencopy_frame_v1: {e}");
            eprintln!(
                "note: it may occur because your wayland compositor does not support this protocol"
            );
            eprintln!(
                "usually it happens on KDE or GNOME. you may use another screenshot utility."
            );
            eprintln!("check compositor support of zwlr_screencopy_frame_v1 here:");
            eprintln!(
                "https://wayland.app/protocols/wlr-screencopy-unstable-v1#compositor-support"
            );
            std::process::exit(1);
        }
        Err(app::Error::Compositor(e)) => {
            eprintln!("failed to initialize wl_compositor: {e}");
            std::process::exit(1);
        }
        Err(app::Error::LayerShell(e)) => {
            eprintln!("failed to initialize layer shell: {e}");
            std::process::exit(1);
        }
        Err(app::Error::Global(e)) => {
            eprintln!("failed to initialize event queue: {e}");
            std::process::exit(1);
        }
        Err(app::Error::CreatePool(e)) => {
            eprintln!("failed to create pool: {e}");
            std::process::exit(1);
        }
        Err(app::Error::Dispatch(e)) => {
            eprintln!("dispatch error: {e}");
            std::process::exit(1);
        }
        Err(app::Error::NoOutput | app::Error::NoOutputInfo) => {
            eprintln!("failed to find any wayland outputs");
            eprintln!("you may turn on your monitor *joke*");
            std::process::exit(1);
        }
        Err(app::Error::NoOutputLogicalSize) => {
            eprintln!("output does not contains information about logical size");
            std::process::exit(1);
        }
    };

    // Write Xrgb8888 buffer to rgb vector
    let mut data = Vec::with_capacity(rect.width as usize * rect.height as usize * 4);

    let region = image.chunks_exact(4);
    let region = region.chunks(width as usize);
    let region = region
        .into_iter()
        .skip(rect.start.y as usize)
        .take(rect.height as usize)
        .flat_map(|v| v.skip(rect.start.x as usize).take(rect.width as usize));

    for chunk in region {
        data.push(chunk[2]);
        data.push(chunk[1]);
        data.push(chunk[0]);
    }

    let buffer = ImageBuffer::<Rgb<u8>, _>::from_raw(rect.width, rect.height, &data[..])
        .expect("Failed to create ImageBuffer from raw data");

    if let Err(e) = buffer.save(&args.output) {
        eprintln!("failed to save: {e}");
    } else {
        println!("saved to {}", args.output);
    }
}
