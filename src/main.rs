use app::{screenshot::ScreenshotApp, AppState, WaylandAppManager};
use clap::Parser;
use image::{codecs::png::PngEncoder, ImageBuffer, ImageError, Rgb};
use iter_tools::Itertools;
use points::{Point, Rectangle};
use rect_fmt::RectFmt;
use wayland_client::Connection;

mod app;
mod points;
mod rect_fmt;

/// Wayland screenshot utility
#[derive(Parser)]
#[command(about, version, after_help = include_str!("../formatting.txt"))]
struct Args {
    /// File to save screenshot (use '-' to output to stdout)
    #[arg(long, short, default_value = "image.png")]
    output: String,

    /// Do not use region selector
    #[arg(long, short)]
    fullscreen: bool,

    /// Only make region selection and print it
    #[arg(long, short)]
    selection_only: bool,

    /// If --selection-only, format of selection output
    #[arg(long, short = 'F', default_value = "%x,%y %wx%h%n")]
    selection_format: String,
}

enum ScreenshotResult {
    Selection {
        image: Box<[u8]>,
        rect: Rectangle,
        width: u32,
        output_name: Option<String>,
    },
    Canceled,
}

fn make_screenshot(args: &Args) -> Result<ScreenshotResult, app::Error> {
    let conn = Connection::connect_to_env().map_err(app::Error::Connect)?;
    // Initialize outputs
    let mut mgr = WaylandAppManager::initialize(&conn)?;

    let output_name = {
        let ctx = mgr.app.ctx.base();
        ctx.output_state
            .outputs()
            .next()
            .and_then(|o| ctx.output_state.info(&o).and_then(|i| i.name))
    };

    // Make screenshot
    mgr.initialize_partial()?;
    mgr.next_app()?;
    mgr.dispatch_until_done()?;

    if args.fullscreen {
        let AppState::ScreenshotApp(ScreenshotApp {
            image: Some(image), ..
        }) = mgr.app.state
        else {
            unreachable!("next app after base should be screenshot, image should be present")
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
            output_name,
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

        Ok(ScreenshotResult::Selection {
            image,
            rect,
            width,
            output_name,
        })
    }
}

fn save_image(args: &Args, rect: Rectangle, data: &[u8]) -> Result<(), ImageError> {
    let buffer = ImageBuffer::<Rgb<u8>, _>::from_raw(rect.width, rect.height, data)
        .expect("Failed to create ImageBuffer from raw data");

    match args.output.as_str() {
        "-" => {
            let encoder = PngEncoder::new(std::io::stdout());
            buffer.write_with_encoder(encoder)?;
        }
        path => {
            buffer.save(path)?;
            println!("saved to {}", args.output);
        }
    }

    Ok(())
}

fn main() {
    let args = Args::parse();

    let (image, rect, width, output_name) = match make_screenshot(&args) {
        Ok(ScreenshotResult::Selection {
            image,
            rect,
            width,
            output_name,
        }) => (image, rect, width, output_name),
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

    if args.selection_only {
        let fmt = RectFmt {
            rect,
            fmt: &args.selection_format,
            output_name: output_name.as_deref(),
        };
        print!("{fmt}");
        std::process::exit(0);
    }

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

    if let Err(e) = save_image(&args, rect, &data) {
        eprintln!("failed to save: {e}");
    }
}
