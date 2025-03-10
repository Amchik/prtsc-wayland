use clap::Parser;
use image::{ImageBuffer, Rgb};
use iter_tools::Itertools;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_keyboard, delegate_layer, delegate_output, delegate_pointer,
    delegate_registry, delegate_seat, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers},
        pointer::{
            cursor_shape::CursorShapeManager, PointerEvent, PointerEventKind, PointerHandler,
        },
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
    shm::{
        slot::{Buffer, SlotPool},
        Shm, ShmHandler,
    },
};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, Dispatch, QueueHandle,
};
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape;
use wayland_protocols_wlr::screencopy::v1::client::{
    zwlr_screencopy_frame_v1::{self, ZwlrScreencopyFrameV1},
    zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1,
};

mod outputs;
mod points;

use points::{ByTwoPoints, Point, PointInt, Rectangle};

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

    let conn = Connection::connect_to_env().unwrap();

    let (globals, mut event_queue) = registry_queue_init(&conn).unwrap();
    let qh = event_queue.handle();

    let output_state = { outputs::output_state(&conn) };
    let output = output_state.outputs().next().expect("at least one output");

    let (width, height) = {
        let info = output_state.info(&output).expect("output info");
        let (w, h) = info.logical_size.expect("logical size");

        (w as u32, h as u32)
    };

    let compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor is not available");
    let layer_shell = LayerShell::bind(&globals, &qh).expect("layer shell is not available");
    let shm = Shm::bind(&globals, &qh).expect("wl_shm is not available");

    let surface = compositor.create_surface(&qh);

    let layer =
        layer_shell.create_layer_surface(&qh, surface, Layer::Overlay, Some("prtsc-wayland"), None);
    layer.set_anchor(Anchor::all());
    layer.set_exclusive_zone(-1);
    layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
    layer.set_size(width, height);
    layer.commit();

    let shape_manager = CursorShapeManager::bind(&globals, &qh).ok();

    let pool =
        SlotPool::new(width as usize * height as usize * 4, &shm).expect("Failed to create pool");

    let registry_state = RegistryState::new(&globals);
    let zwlr_screencopy_manager: ZwlrScreencopyManagerV1 = registry_state
        .bind_one(&qh, 1..=3, ())
        .expect("failed to bind zwlr_screencopy_manager_v1");

    let zwlr_screencopy_frame = zwlr_screencopy_manager.capture_output(0, &output, &qh, ());
    let mut app = App {
        registry_state,
        seat_state: SeatState::new(&globals, &qh),
        output_state,
        shape_manager,
        shm,

        exit: false,
        pool,
        width,
        height,
        layer,
        keyboard: None,
        pointer: None,

        buffer: None,
        zwlr_screencopy_frame,
        image: Box::default(),

        state: if args.fullscreen {
            AppState::FullscreenOnly
        } else {
            AppState::default()
        },
    };

    while !app.exit {
        event_queue.blocking_dispatch(&mut app).unwrap();
    }

    drop(conn);

    if let AppState::SelectionCompleted(rect) = app.state {
        let mut data = Vec::with_capacity(rect.width as usize * rect.height as usize * 4);

        let region = app.image.chunks_exact(4);
        let region = region.chunks(app.width as usize);
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
}

/// State of application logic
#[derive(Clone, Debug, Default)]
pub enum AppState {
    /// Like [`FullscreenWait`] but after completing fullscreen
    /// capturing skips all other states and jumps to [`SelectionCompleted`]
    /// with fullscreen rectangle. Used in `--fullscreen` mode.
    /// Doesn't draws anything.
    FullscreenOnly,

    /// First state, waiting for `Ready` event.
    #[default]
    FullscreenWait,
    /// Got `Ready` event, performing first draw
    FullscreenCompleted,
    /// First draw performed, can select something
    SelectionWait,
    /// When one point already choosed, making drawing boxes, etc
    SelectionProcess {
        /// Initial point
        initial: Point,
        /// Previous point of selection
        previous: Point,
        /// Pending point to write
        pending: Option<Point>,
    },
    /// Completed selection, now exiting and saving image
    SelectionCompleted(Rectangle),
}

struct App {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shape_manager: Option<CursorShapeManager>,
    shm: Shm,

    exit: bool,
    pool: SlotPool,
    width: u32,
    height: u32,
    layer: LayerSurface,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,

    buffer: Option<Buffer>,
    zwlr_screencopy_frame: ZwlrScreencopyFrameV1,
    image: Box<[u8]>,

    state: AppState,
}

impl<U> Dispatch<ZwlrScreencopyFrameV1, U> for App {
    fn event(
        state: &mut Self,
        _proxy: &ZwlrScreencopyFrameV1,
        event: <ZwlrScreencopyFrameV1 as wayland_client::Proxy>::Event,
        _data: &U,
        _conn: &Connection,
        qhandle: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_screencopy_frame_v1::Event::Buffer {
                width,
                height,
                stride,
                format,
            } => {
                let format = match format {
                    wayland_client::WEnum::Value(format) => format,
                    wayland_client::WEnum::Unknown(id) => panic!("unsupported format: {id}"),
                };
                state.recreate_buffer(width as i32, height as i32, stride as i32, format);
            }
            zwlr_screencopy_frame_v1::Event::Ready { .. } => {
                let fullscreen_only = matches!(state.state, AppState::FullscreenOnly);

                state.state = AppState::FullscreenCompleted;
                state.save_buffer_to_image();

                if fullscreen_only {
                    state.state = AppState::SelectionCompleted(Rectangle::new(
                        Point::new(0, 0),
                        state.width,
                        state.height,
                    ));
                    state.exit = true;
                } else {
                    state.state = AppState::SelectionWait;
                    state.draw_begin_selection(qhandle);
                }
            }
            _ => {}
        }
    }
}

impl<U> Dispatch<ZwlrScreencopyManagerV1, U> for App {
    fn event(
        _state: &mut Self,
        _proxy: &ZwlrScreencopyManagerV1,
        _event: <ZwlrScreencopyManagerV1 as wayland_client::Proxy>::Event,
        _data: &U,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
    }
}

impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_factor: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
        if let AppState::SelectionProcess {
            pending: Some(pos), ..
        } = &self.state
        {
            let pos = pos.clone();
            if self.draw_in_selection(qh, pos.clone()) {
                match &mut self.state {
                    AppState::SelectionProcess {
                        previous, pending, ..
                    } => {
                        *pending = None;
                        *previous = pos;
                    }
                    _ => unreachable!(),
                }
            }
        }
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }
}

impl LayerShellHandler for App {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer: &LayerSurface) {
        self.exit = true;
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        if configure.new_size.0 == 0 || configure.new_size.1 == 0 {
            self.width = 256;
            self.height = 256;
        } else {
            self.width = configure.new_size.0;
            self.height = configure.new_size.1;
        }
    }
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            let keyboard = self
                .seat_state
                .get_keyboard(qh, &seat, None)
                .expect("Failed to create keyboard");
            self.keyboard = Some(keyboard);
        }

        if capability == Capability::Pointer && self.pointer.is_none() {
            let pointer = self
                .seat_state
                .get_pointer(qh, &seat)
                .expect("Failed to create pointer");
            self.pointer = Some(pointer);
        }
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard && self.keyboard.is_some() {
            self.keyboard.take().unwrap().release();
        }

        if capability == Capability::Pointer && self.pointer.is_some() {
            self.pointer.take().unwrap().release();
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl KeyboardHandler for App {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _surface: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _keysyms: &[Keysym],
    ) {
    }

    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _surface: &wl_surface::WlSurface,
        _: u32,
    ) {
    }

    fn press_key(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        if event.keysym == Keysym::Escape {
            match &self.state {
                AppState::SelectionProcess { .. } => {
                    self.state = AppState::SelectionWait;
                    self.draw_begin_selection(qh);
                }
                _ => {
                    self.exit = true;
                }
            }
        }
    }

    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _event: KeyEvent,
    ) {
    }

    fn update_modifiers(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        _modifiers: Modifiers,
        _layout: u32,
    ) {
    }
}

impl PointerHandler for App {
    fn pointer_frame(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        pointer: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        use PointerEventKind::*;
        for event in events {
            // Ignore events for other surfaces
            if &event.surface != self.layer.wl_surface() {
                continue;
            }
            let pos = Point::new(event.position.0 as PointInt, event.position.1 as PointInt);
            match event.kind {
                Enter { serial } => {
                    if let Some(shape_manager) = &self.shape_manager {
                        let dev = shape_manager.get_shape_device(pointer, qh);
                        dev.set_shape(serial, Shape::Crosshair);
                    }
                }
                Motion { .. } => {
                    if let AppState::SelectionProcess { previous, .. } = &self.state {
                        if previous != &pos {
                            if self.draw_in_selection(qh, pos.clone()) {
                                match &mut self.state {
                                    AppState::SelectionProcess { previous, .. } => {
                                        *previous = pos;
                                    }
                                    _ => unreachable!(),
                                }
                            } else {
                                match &mut self.state {
                                    AppState::SelectionProcess { pending, .. } => {
                                        *pending = Some(pos);
                                    }
                                    _ => unreachable!(),
                                }
                            }
                        }
                    }
                }
                Press { button: 272, .. } => {
                    self.state = AppState::SelectionProcess {
                        initial: pos.clone(),
                        previous: pos,
                        pending: None,
                    };
                }
                Release { button: 272, .. } => {
                    let (init, current) = match &self.state {
                        AppState::SelectionProcess {
                            initial, previous, ..
                        } => (initial, previous),
                        _ => return,
                    };
                    let Some(rect) = Rectangle::from_two_points(init.clone(), current.clone())
                    else {
                        eprintln!("selected zero-area region, exiting");
                        std::process::exit(1);
                    };
                    self.exit = true;
                    self.state = AppState::SelectionCompleted(rect);
                }
                _ => {}
            }
        }
    }
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

fn dim_u8(src: u8) -> u8 {
    const DIM_FACTOR: u8 = 128;

    (src as usize * DIM_FACTOR as usize / 256) as u8
}

impl App {
    pub fn recreate_buffer(
        &mut self,
        width: i32,
        height: i32,
        stride: i32,
        format: wl_shm::Format,
    ) {
        self.width = width as u32;
        self.height = height as u32;
        self.buffer = Some({
            let (buffer, _canvas) = self
                .pool
                .create_buffer(width, height, stride, format)
                .expect("create buffer");

            self.zwlr_screencopy_frame.copy(buffer.wl_buffer());

            buffer
        });
    }

    fn crosshair_draw(
        canvas: &mut [u8],
        pos: &Point,
        width: u32,
        height: u32,
        layer: &LayerSurface,
    ) {
        // Horizontal line
        for ptr in 0..height {
            let ptr = (pos.x + ptr * width) as usize * 4;
            canvas[ptr] = 255;
            canvas[ptr + 1] = 255;
            canvas[ptr + 2] = 255;
            canvas[ptr + 3] = 255;
        }
        // Vertical line
        canvas[(width * pos.y) as usize * 4..(width * (pos.y + 1)) as usize * 4].fill(255);

        layer
            .wl_surface()
            .damage_buffer(pos.x as i32, 0, 1, height as i32);
        layer
            .wl_surface()
            .damage_buffer(0, pos.y as i32, width as i32, 1);
    }

    fn crosshair_clear(
        canvas: &mut [u8],
        pos: &Point,
        width: u32,
        height: u32,
        layer: &LayerSurface,
        image: &[u8],
    ) {
        for ptr in 0..height {
            let ptr = (pos.x + ptr * width) as usize * 4;
            canvas[ptr] = dim_u8(image[ptr]);
            canvas[ptr + 1] = dim_u8(image[ptr + 1]);
            canvas[ptr + 2] = dim_u8(image[ptr + 2]);
            canvas[ptr + 3] = dim_u8(image[ptr + 3]);
        }
        for ptr in width * pos.y..width * (pos.y + 1) {
            let ptr = ptr as usize * 4;
            canvas[ptr] = dim_u8(image[ptr]);
            canvas[ptr + 1] = dim_u8(image[ptr + 1]);
            canvas[ptr + 2] = dim_u8(image[ptr + 2]);
            canvas[ptr + 3] = dim_u8(image[ptr + 3]);
        }

        layer
            .wl_surface()
            .damage_buffer(pos.x as i32, 0, 1, height as i32);
        layer
            .wl_surface()
            .damage_buffer(0, pos.y as i32, width as i32, 1);
    }

    fn dim_rect(
        rect: Rectangle,
        canvas: &mut [u8],
        image: &[u8],
        width: usize,
        layer: &LayerSurface,
    ) {
        for col in rect.start.x..(rect.start.x + rect.width) {
            for row in rect.start.y..(rect.start.y + rect.height) {
                let pos = row as usize * width + col as usize;
                canvas[pos * 4] = dim_u8(image[pos * 4]);
                canvas[pos * 4 + 1] = dim_u8(image[pos * 4 + 1]);
                canvas[pos * 4 + 2] = dim_u8(image[pos * 4 + 2]);
                canvas[pos * 4 + 3] = dim_u8(image[pos * 4 + 3]);
            }
        }

        layer.wl_surface().damage_buffer(
            rect.start.x as i32,
            rect.start.y as i32,
            rect.width as i32,
            rect.height as i32,
        );
    }

    pub fn draw_in_selection(&mut self, qh: &QueueHandle<Self>, pos: Point) -> bool {
        let buffer = self.buffer.as_mut().expect("non-ready buffer");
        let canvas = match self.pool.canvas(buffer) {
            Some(canvas) => canvas,
            None => {
                return false;
            }
        };

        let image = &self.image;

        let (init, prev) = match &self.state {
            AppState::SelectionProcess {
                initial, previous, ..
            } => (initial, previous),
            _ => unreachable!("called draw_in_selection on incorrect state"),
        };

        Self::crosshair_clear(
            canvas,
            prev,
            self.width,
            self.height,
            &self.layer,
            &self.image,
        );

        if init.is_same_quater(&pos, prev) {
            // NOTE: rectangle (prev) -> (pos) is rewriting twice.
            if let Some(rect) = Rectangle::from_two_points(pos.clone(), prev.clone()) {
                Self::dim_rect(rect, canvas, image, self.width as usize, &self.layer);
            }

            let axis_x = Point::new(prev.x, init.y);
            if let Some(rect) = Rectangle::from_two_points(pos.clone(), axis_x) {
                Self::dim_rect(rect, canvas, image, self.width as usize, &self.layer);
            }

            let axis_y = Point::new(init.x, prev.y);
            if let Some(rect) = Rectangle::from_two_points(pos.clone(), axis_y) {
                Self::dim_rect(rect, canvas, image, self.width as usize, &self.layer);
            }
        } else if let Some(rect) = Rectangle::from_two_points(init.clone(), prev.clone()) {
            Self::dim_rect(rect, canvas, image, self.width as usize, &self.layer);
        }

        if let ByTwoPoints::Rectangle(rect) = init.clone().into_figure(pos.clone()) {
            for row in rect.start.y..rect.start.y + rect.height {
                let row = (self.width * row) as usize * 4;
                let start = row + rect.start.x as usize * 4;
                let end = start + rect.width as usize * 4;
                canvas[start..end].copy_from_slice(&image[start..end]);
            }
            self.layer.wl_surface().damage_buffer(
                rect.start.x as i32,
                rect.start.y as i32,
                rect.width as i32,
                rect.height as i32,
            );
        }

        Self::crosshair_draw(canvas, init, self.width, self.height, &self.layer);
        Self::crosshair_draw(canvas, &pos, self.width, self.height, &self.layer);

        let surface = self.layer.wl_surface();

        // Request our next frame
        self.layer.wl_surface().frame(qh, surface.clone());

        // Attach and commit to present.
        buffer.attach_to(surface).expect("buffer attach");
        self.layer.commit();

        true
    }

    pub fn draw_begin_selection(&mut self, qh: &QueueHandle<Self>) {
        // Assert that we call this function in correct state.
        debug_assert!(matches!(self.state, AppState::SelectionWait));

        let (buffer, canvas) = {
            self.buffer = None;
            let canvas;
            let buffer = self.buffer.insert({
                let (buffer, new_canvas) = self
                    .pool
                    .create_buffer(
                        self.width as i32,
                        self.height as i32,
                        self.width as i32 * 4,
                        wl_shm::Format::Xrgb8888,
                    )
                    .expect("buffer");
                canvas = new_canvas;
                buffer
            });

            (buffer, canvas)
        };

        canvas
            .iter_mut()
            .zip(self.image.iter())
            .for_each(|(dst, &src)| *dst = dim_u8(src));

        // Damage the entire window
        self.layer
            .wl_surface()
            .damage_buffer(0, 0, self.width as i32, self.height as i32);

        // Request our next frame
        self.layer
            .wl_surface()
            .frame(qh, self.layer.wl_surface().clone());

        // Attach and commit to present.
        buffer
            .attach_to(self.layer.wl_surface())
            .expect("buffer attach");
        self.layer.commit();
    }

    pub fn save_buffer_to_image(&mut self) {
        let buffer = self
            .buffer
            .as_ref()
            .expect("called draw() on non-ready buffer");
        let slot = buffer.slot();
        let data = self.pool.raw_data_mut(&slot);

        self.image = data.to_vec().into_boxed_slice();
    }
}

delegate_compositor!(App);
delegate_output!(App);
delegate_shm!(App);

delegate_seat!(App);
delegate_keyboard!(App);
delegate_pointer!(App);

delegate_layer!(App);

delegate_registry!(App);

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}
