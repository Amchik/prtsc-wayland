use app::{OldWaylandApp, OldWaylandContext, WaylandAppManager};
use clap::Parser;
use image::{ImageBuffer, Rgb};
use iter_tools::Itertools;
use wayland_client::Connection;

mod app;
mod outputs;
mod points;
mod dbg_time;

#[derive(Parser)]
struct Args {
    /// File to save screenshot
    #[arg(long, short, default_value = "image.png")]
    output: String,

    /// Do not use region selector
    #[arg(long, short)]
    fullscreen: bool,
}

fn main() {
    let args = Args::parse();

    // New (idiot)
    let conn = Connection::connect_to_env().unwrap();
    let mut mgr = WaylandAppManager::initialize(&conn).unwrap();
    mgr.initialize_partial().unwrap();
    mgr.next_app().unwrap();
    mgr.dispatch_until_done().unwrap();
    mgr.initialize_full().unwrap();
    mgr.next_app().unwrap();
    mgr.dispatch_until_done().unwrap();
    mgr.next_app().unwrap(); // panic

    // Old (buggy)
    let (image, rect, width) = {
        let conn = Connection::connect_to_env().unwrap();
        let ctx = OldWaylandContext::new(conn);

        let app: OldWaylandApp<app::prepare::PrepareApp> = ctx.init_from(()).unwrap();
        let mut app: OldWaylandApp<app::screenshot::OldScreenshotApp> =
            ctx.init_from(app.into_app()).unwrap();
        app.dispatch_until_done().unwrap();

        let mut app: OldWaylandApp<app::selection::OldSelectionApp> =
            ctx.init_from(app.into_app()).unwrap();
        println!("here");
        app.dispatch_until_done().unwrap();
        println!("here");

        let app = app.into_app();

        let rect = app.get_selection();

        (app.image, rect, app.width)
    };
//
//    if let Some(rect) = rect {
//        let mut data = Vec::with_capacity(rect.width as usize * rect.height as usize * 4);
//
//        let region = image.chunks_exact(4);
//        let region = region.chunks(width as usize);
//        let region = region
//            .into_iter()
//            .skip(rect.start.y as usize)
//            .take(rect.height as usize)
//            .flat_map(|v| v.skip(rect.start.x as usize).take(rect.width as usize));
//
//        for chunk in region {
//            data.push(chunk[2]);
//            data.push(chunk[1]);
//            data.push(chunk[0]);
//        }
//
//        let buffer = ImageBuffer::<Rgb<u8>, _>::from_raw(rect.width, rect.height, &data[..])
//            .expect("Failed to create ImageBuffer from raw data");
//
//        if let Err(e) = buffer.save(&args.output) {
//            eprintln!("failed to save: {e}");
//        } else {
//            println!("saved to {}", args.output);
//        }
//    } else {
//        eprintln!("invalid selection");
//    }
}
