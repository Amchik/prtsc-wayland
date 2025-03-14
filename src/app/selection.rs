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
    globals::{registry_queue_init, BindError, GlobalError, GlobalList},
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, Dispatch, DispatchError, EventQueue, QueueHandle,
};
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape;

use crate::points::{ByTwoPoints, Point, PointInt, Rectangle};

use super::{
    screenshot::OldScreenshotApp, DispatchResult, DispatchWhile, InitFrom, StatePhase, WaylandApp,
    WaylandAppState, WaylandAppStateFromPrevious, WaylandContext,
};

#[derive(Default)]
enum SelectionState {
    #[default]
    Waiting,
    BeginSelection {
        initial: Point,
        current: Point,
        pending: Option<Point>,
    },
    SelectionCompleted(Rectangle),
    Abort,
}

pub struct SelectionApp {
    pub image: Box<[u8]>,
    pub buffer: Buffer,

    state: SelectionState,
}

impl WaylandAppStateFromPrevious for SelectionApp {
    type Previous = super::screenshot::ScreenshotApp;

    fn from_previous(
        context: &mut super::WaylandContext,
        previous: Self::Previous,
        _: &GlobalList,
        _: &mut EventQueue<WaylandApp>,
    ) -> Result<Self, super::Error> {
        let image = previous.image.expect(
            "attempt to switch state on non-completed phase, no image present from screenshot app",
        );
        let partial = context
            .partial_mut()
            .expect("SelectionApp requires at least partial context");

        let (width, height) = {
            let pos = partial.logical_size.clone();

            (pos.x, pos.y)
        };

        let (buffer, canvas) = partial
            .pool
            .create_buffer(
                width as i32,
                height as i32,
                width as i32 * 4,
                wl_shm::Format::Xrgb8888,
            )
            .expect("failed to create buffer format xrgb8888");

        Ok(Self {
            image,
            buffer,
            state: Default::default(),
        })
    }
}

impl WaylandAppState for SelectionApp {
    fn current_phase(&self) -> StatePhase {
        match &self.state {
            SelectionState::Abort | SelectionState::SelectionCompleted(_) => StatePhase::Done,
            _ => StatePhase::Active,
        }
    }

    fn on_key_press(
        &mut self,
        ctx: &mut WaylandContext,
        event: KeyEvent,
        qh: &QueueHandle<WaylandApp>,
    ) {
        if event.keysym == Keysym::Escape {
            if let SelectionState::Waiting = self.state {
                self.state = SelectionState::Abort;
            } else {
                self.state = SelectionState::Waiting;
                self.on_redraw(ctx, qh);
            }
        }
    }

    fn on_mouse_enter(
        &mut self,
        ctx: &mut WaylandContext,
        _pos: Point,
        pointer: &wl_pointer::WlPointer,
        serial: u32,
        qh: &QueueHandle<WaylandApp>,
    ) {
        if let Some(shape_manager) = ctx.full().and_then(|v| v.shape_manager.as_ref()) {
            let dev = shape_manager.get_shape_device(pointer, qh);
            dev.set_shape(serial, Shape::Crosshair);
        }
    }

    fn on_mouse_move(
        &mut self,
        ctx: &mut WaylandContext,
        pos: Point,
        qh: &QueueHandle<WaylandApp>,
    ) {
        if let SelectionState::BeginSelection { pending, .. } = &mut self.state {
            *pending = Some(pos);
            self.on_redraw(ctx, qh);
        }
    }
    fn on_mouse_press(
        &mut self,
        _ctx: &mut WaylandContext,
        pos: Point,
        _qh: &QueueHandle<WaylandApp>,
    ) {
        let SelectionState::Waiting = self.state else {
            return;
        };

        self.state = SelectionState::BeginSelection {
            initial: pos.clone(),
            current: pos,
            pending: None,
        };
    }
    fn on_mouse_release(
        &mut self,
        _ctx: &mut WaylandContext,
        _pos: Point,
        _qh: &QueueHandle<WaylandApp>,
    ) {
        let SelectionState::BeginSelection {
            initial,
            current,
            pending: _,
        } = &self.state
        else {
            return;
        };

        let rect = Rectangle::from_two_points(initial.clone(), current.clone());
        let Some(rect) = rect else {
            // BUG: this code shouldn't panic. Need to set selection state to errnous or at least
            // aborted/waiting.
            todo!("set selection state to errnous if rectangle is degenerated")
        };

        self.state = SelectionState::SelectionCompleted(rect);
    }

    /// Called on random redraws and on mouse movement
    fn on_redraw(&mut self, ctx: &mut WaylandContext, qh: &QueueHandle<WaylandApp>) {
        let buffer = &mut self.buffer;
        let (canvas, layer, width, height) = {
            let ctx = ctx
                .full_mut()
                .expect("SelectionApp requires full context to draw");

            let canvas = match ctx.partial.pool.canvas(buffer) {
                Some(canvas) => canvas,
                None => return,
            };

            let layer = &ctx.layer;
            let pos = ctx.partial.logical_size.clone();

            (canvas, layer, pos.x, pos.y)
        };

        let (init, previous, pending) = match &mut self.state {
            SelectionState::BeginSelection {
                initial,
                current,
                pending: Some(pending),
            } if current != pending => (initial.clone(), current, pending.clone()),

            SelectionState::Waiting => {
                utils::dim_rect(
                    Rectangle::new(Point::new(0, 0), width, height),
                    canvas,
                    &self.image,
                    width as usize,
                    Some(layer),
                );
                utils::commit_drawing(layer, buffer, qh);
                return;
            }

            _ => return,
        };

        utils::dim_crosshair(
            previous.clone(),
            canvas,
            &self.image,
            width,
            height,
            Some(layer),
        );

        // FIXME: this implementation just dim rect(init, previous) and shows rect(init, pending)
        // with too many redraws
        if let Some(rect) = Rectangle::from_two_points(init.clone(), previous.clone()) {
            utils::dim_rect(rect, canvas, &self.image, width as usize, Some(layer));
        }
        if let Some(rect) = Rectangle::from_two_points(init.clone(), pending.clone()) {
            utils::copy_rect(rect, canvas, &self.image, width as usize, Some(layer));
        }

        utils::fill_crosshair(init, canvas, width, height, Some(layer));
        utils::fill_crosshair(pending.clone(), canvas, width, height, Some(layer));

        *previous = pending;

        utils::commit_drawing(layer, buffer, qh);
    }
}

pub struct OldSelectionApp {
    pub registry_state: RegistryState,
    pub output_state: OutputState,

    pub seat_state: SeatState,
    pub shape_manager: Option<CursorShapeManager>,
    pub keyboard: Option<wl_keyboard::WlKeyboard>,
    pub pointer: Option<wl_pointer::WlPointer>,

    pub shm: Shm,
    pub layer: LayerSurface,
    pub pool: SlotPool,

    pub image: Box<[u8]>,
    pub width: u32,
    pub height: u32,

    pub buffer: Buffer,

    state: SelectionState,
}

#[derive(Debug)]
pub enum Error {
    Compositor(BindError),
    Layer(BindError),
}

impl OldSelectionApp {
    pub fn get_selection(&self) -> Option<Rectangle> {
        match &self.state {
            SelectionState::SelectionCompleted(r) => Some(r.clone()),
            _ => None,
        }
    }
}

impl InitFrom<OldScreenshotApp> for OldSelectionApp {
    type Error = Error;

    fn init(
        globals: &GlobalList,
        event_queue: &mut EventQueue<Self>,
        OldScreenshotApp {
            registry_state: _,
            output_state: _,
            shm: _,
            pool: _,
            width,
            height,
            buffer: image,
            ..
        }: OldScreenshotApp,
    ) -> Result<Self, Self::Error> {
        let image = image.unwrap();

        let qh = event_queue.handle();

        let output_state = OutputState::new(globals, &qh);
        let registry_state = RegistryState::new(globals);

        let seat_state = SeatState::new(globals, &qh);
        let shape_manager = CursorShapeManager::bind(globals, &qh).ok();

        let compositor = CompositorState::bind(globals, &qh).map_err(Error::Compositor)?;
        let layer_shell = LayerShell::bind(globals, &qh).map_err(Error::Layer)?;

        let shm = Shm::bind(globals, &qh).expect("wl_shm is not available");
        let mut pool = SlotPool::new(width as usize * height as usize * 4, &shm)
            .expect("Failed to create pool");

        let surface = compositor.create_surface(&qh);
        let layer = layer_shell.create_layer_surface(
            &qh,
            surface,
            Layer::Overlay,
            Some("prtsc-wayland"),
            None,
        );
        layer.set_anchor(const { Anchor::all() });
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        layer.set_size(width, height);
        layer.commit();

        let (buffer, canvas) = pool
            .create_buffer(
                width as i32,
                height as i32,
                width as i32 * 4,
                wl_shm::Format::Xrgb8888,
            )
            .expect("failed to create buffer format xrgb8888");

        // Dim entire screen. Don't damage layer because we didn't draw anyway
        utils::dim_rect(
            Rectangle::new(Point::new(0, 0), width, height),
            canvas,
            &image,
            width as usize,
            None,
        );

        Ok(Self {
            image,
            registry_state,
            width,
            height,
            pool,
            shm,
            output_state,
            layer,
            seat_state,
            buffer,
            pointer: None,
            keyboard: None,
            shape_manager,
            state: Default::default(),
        })
    }
}

impl DispatchWhile for OldSelectionApp {
    type Error = DispatchError;

    fn dispatch(
        &mut self,
        _globals: &GlobalList,
        event_queue: &mut EventQueue<Self>,
    ) -> Result<DispatchResult, Self::Error> {
        event_queue.blocking_dispatch(self)?;

        match &self.state {
            SelectionState::Abort | SelectionState::SelectionCompleted(_) => {
                Ok(DispatchResult::Done)
            }
            _ => Ok(DispatchResult::Continue),
        }
    }
}

impl OldSelectionApp {
    /// Action on `Esc` key pressed
    fn on_escape(&mut self, _qh: &QueueHandle<Self>) {
        if let SelectionState::Waiting = self.state {
            self.state = SelectionState::Abort;
        } else {
            self.state = SelectionState::Waiting;
        }
    }

    fn on_mouse_move(&mut self, pos: Point, qh: &QueueHandle<Self>) {
        if let SelectionState::BeginSelection { pending, .. } = &mut self.state {
            *pending = Some(pos);
            self.on_redraw(qh);
        }
    }
    fn on_mouse_press(&mut self, pos: Point, _qh: &QueueHandle<Self>) {
        let SelectionState::Waiting = self.state else {
            return;
        };

        self.state = SelectionState::BeginSelection {
            initial: pos.clone(),
            current: pos,
            pending: None,
        };
    }
    fn on_mouse_release(&mut self, _pos: Point, _qh: &QueueHandle<Self>) {
        let SelectionState::BeginSelection {
            initial,
            current,
            pending: _,
        } = &self.state
        else {
            return;
        };

        let rect = Rectangle::from_two_points(initial.clone(), current.clone());
        let Some(rect) = rect else {
            // BUG: this code shouldn't panic. Need to set selection state to errnous or at least
            // aborted/waiting.
            todo!("set selection state to errnous if rectangle is degenerated")
        };

        self.state = SelectionState::SelectionCompleted(rect);
    }

    /// Called on random redraws and on mouse movement
    fn on_redraw(&mut self, qh: &QueueHandle<Self>) {
        println!("called on_redraw");

        let buffer = &mut self.buffer;
        let canvas = match self.pool.canvas(buffer) {
            Some(canvas) => canvas,
            None => return,
        };

        let (init, previous, pending) = match &mut self.state {
            SelectionState::BeginSelection {
                initial,
                current,
                pending: Some(pending),
            } if current != pending => (initial.clone(), current, pending.clone()),

            SelectionState::Waiting => {
                utils::dim_rect(
                    Rectangle::new(Point::new(0, 0), self.width, self.height),
                    canvas,
                    &self.image,
                    self.width as usize,
                    Some(&self.layer),
                );
                Self::draw_commit(&self.layer, buffer, qh);
                return;
            }

            _ => return,
        };

        utils::dim_crosshair(
            previous.clone(),
            canvas,
            &self.image,
            self.width,
            self.height,
            Some(&self.layer),
        );

        // FIXME: this implementation just dim rect(init, previous) and shows rect(init, pending)
        // with too many redraws
        /*if let Some(rect) = Rectangle::from_two_points(init.clone(), previous.clone()) {
            utils::dim_rect(
                rect,
                canvas,
                &self.image,
                self.width as usize,
                Some(&self.layer),
            );
        }
        if let Some(rect) = Rectangle::from_two_points(init.clone(), pending.clone()) {
            utils::copy_rect(
                rect,
                canvas,
                &self.image,
                self.width as usize,
                Some(&self.layer),
            );
        }*/
        // <<<
        if init.is_same_quater(&pending, previous) {
            // NOTE: rectangle (prev) -> (pos) is rewriting twice.
            if let Some(rect) = Rectangle::from_two_points(pending.clone(), previous.clone()) {
                utils::dim_rect(
                    rect,
                    canvas,
                    &self.image,
                    self.width as usize,
                    Some(&self.layer),
                );
            }

            let axis_x = Point::new(previous.x, init.y);
            if let Some(rect) = Rectangle::from_two_points(pending.clone(), axis_x) {
                utils::dim_rect(
                    rect,
                    canvas,
                    &self.image,
                    self.width as usize,
                    Some(&self.layer),
                );
            }

            let axis_y = Point::new(init.x, previous.y);
            if let Some(rect) = Rectangle::from_two_points(pending.clone(), axis_y) {
                utils::dim_rect(
                    rect,
                    canvas,
                    &self.image,
                    self.width as usize,
                    Some(&self.layer),
                );
            }
        } else if let Some(rect) = Rectangle::from_two_points(init.clone(), previous.clone()) {
            utils::dim_rect(
                rect,
                canvas,
                &self.image,
                self.width as usize,
                Some(&self.layer),
            );
        }

        if let ByTwoPoints::Rectangle(rect) = init.clone().into_figure(pending.clone()) {
            utils::copy_rect(
                rect,
                canvas,
                &self.image,
                self.width as usize,
                Some(&self.layer),
            );
        }

        utils::fill_crosshair(
            previous.clone(),
            canvas,
            self.width,
            self.height,
            Some(&self.layer),
        );
        utils::fill_crosshair(
            pending.clone(),
            canvas,
            self.width,
            self.height,
            Some(&self.layer),
        );

        *previous = pending;

        Self::draw_commit(&self.layer, buffer, qh);
    }

    fn draw_commit(layer: &LayerSurface, buffer: &Buffer, qh: &QueueHandle<Self>) {
        let surface = layer.wl_surface();

        // Request our next frame
        layer.wl_surface().frame(qh, surface.clone());

        // Attach and commit to present.
        buffer.attach_to(surface).expect("buffer attach");
        layer.commit();
    }
}

mod utils {
    use smithay_client_toolkit::{
        shell::{wlr_layer::LayerSurface, WaylandSurface},
        shm::slot::Buffer,
    };
    use wayland_client::QueueHandle;

    use crate::{
        app::WaylandApp,
        points::{Point, Rectangle},
    };

    pub fn commit_drawing(layer: &LayerSurface, buffer: &Buffer, qh: &QueueHandle<WaylandApp>) {
        let surface = layer.wl_surface();

        // Request our next frame
        layer.wl_surface().frame(qh, surface.clone());

        // Attach and commit to present.
        buffer.attach_to(surface).expect("buffer attach");
        layer.commit();
    }

    pub fn copy_rect(
        rect: Rectangle,
        canvas: &mut [u8],
        image: &[u8],
        width: usize,
        layer: Option<&LayerSurface>,
    ) {
        for row in rect.start.y..rect.start.y + rect.height {
            let row = width * row as usize * 4;
            let start = row + rect.start.x as usize * 4;
            let end = start + rect.width as usize * 4;
            canvas[start..end].copy_from_slice(&image[start..end]);
        }
        if let Some(layer) = layer {
            layer.wl_surface().damage_buffer(
                rect.start.x as i32,
                rect.start.y as i32,
                rect.width as i32,
                rect.height as i32,
            );
        }
    }

    pub fn dim_u8(src: u8) -> u8 {
        const DIM_FACTOR: u8 = 128;

        (src as usize * DIM_FACTOR as usize / 256) as u8
    }

    pub fn dim_rect(
        rect: Rectangle,
        canvas: &mut [u8],
        image: &[u8],
        width: usize,
        layer: Option<&LayerSurface>,
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

        if let Some(layer) = layer {
            layer.wl_surface().damage_buffer(
                rect.start.x as i32,
                rect.start.y as i32,
                rect.width as i32,
                rect.height as i32,
            );
        }
    }

    pub fn dim_crosshair(
        pos: Point,
        canvas: &mut [u8],
        image: &[u8],
        width: u32,
        height: u32,
        layer: Option<&LayerSurface>,
    ) {
        // Vertical line
        for ptr in 0..height {
            let ptr = (pos.x + ptr * width) as usize * 4;
            canvas[ptr] = dim_u8(image[ptr]);
            canvas[ptr + 1] = dim_u8(image[ptr + 1]);
            canvas[ptr + 2] = dim_u8(image[ptr + 2]);
            canvas[ptr + 3] = dim_u8(image[ptr + 3]);
        }
        // Horizontal line
        for ptr in width * pos.y..width * (pos.y + 1) {
            let ptr = ptr as usize * 4;
            canvas[ptr] = dim_u8(image[ptr]);
            canvas[ptr + 1] = dim_u8(image[ptr + 1]);
            canvas[ptr + 2] = dim_u8(image[ptr + 2]);
            canvas[ptr + 3] = dim_u8(image[ptr + 3]);
        }

        if let Some(layer) = layer {
            layer
                .wl_surface()
                .damage_buffer(pos.x as i32, 0, 1, height as i32);
            layer
                .wl_surface()
                .damage_buffer(0, pos.y as i32, width as i32, 1);
        }
    }

    pub fn fill_crosshair(
        pos: Point,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        layer: Option<&LayerSurface>,
    ) {
        // Vertical line
        for ptr in 0..height {
            let ptr = (pos.x + ptr * width) as usize * 4;
            canvas[ptr] = 255;
            canvas[ptr + 1] = 255;
            canvas[ptr + 2] = 255;
            canvas[ptr + 3] = 255;
        }
        // Horizontal line
        canvas[(width * pos.y) as usize * 4..(width * (pos.y + 1)) as usize * 4].fill(255);

        if let Some(layer) = layer {
            layer
                .wl_surface()
                .damage_buffer(pos.x as i32, 0, 1, height as i32);
            layer
                .wl_surface()
                .damage_buffer(0, pos.y as i32, width as i32, 1);
        }
    }
}

impl KeyboardHandler for OldSelectionApp {
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
            self.on_escape(qh);
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

impl PointerHandler for OldSelectionApp {
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
                    self.on_mouse_move(pos, qh);
                }
                Press { button: 272, .. } => {
                    self.on_mouse_press(pos, qh);
                }
                Release { button: 272, .. } => {
                    self.on_mouse_release(pos, qh);
                }
                _ => {}
            }
        }
    }
}

impl SeatHandler for OldSelectionApp {
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

impl CompositorHandler for OldSelectionApp {
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
        self.on_redraw(qh);
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

impl LayerShellHandler for OldSelectionApp {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer: &LayerSurface) {
        self.state = SelectionState::Abort;
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _layer: &LayerSurface,
        _configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        // idk what is that lol
    }
}

impl OutputHandler for OldSelectionApp {
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
impl ShmHandler for OldSelectionApp {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

delegate_seat!(OldSelectionApp);
delegate_keyboard!(OldSelectionApp);
delegate_pointer!(OldSelectionApp);

delegate_output!(OldSelectionApp);
delegate_compositor!(OldSelectionApp);
delegate_shm!(OldSelectionApp);
delegate_layer!(OldSelectionApp);

delegate_registry!(OldSelectionApp);

impl ProvidesRegistryState for OldSelectionApp {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}
