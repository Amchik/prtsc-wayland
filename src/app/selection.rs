use smithay_client_toolkit::{
    seat::keyboard::{KeyEvent, Keysym},
    shm::slot::Buffer,
};
use wayland_client::{
    globals::GlobalList,
    protocol::{wl_pointer, wl_shm},
    EventQueue, QueueHandle,
};
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape;

use crate::points::{Point, Rectangle};

use super::{StatePhase, WaylandApp, WaylandAppState, WaylandAppStateFromPrevious, WaylandContext};

struct SelectionData {
    pub initial: Point,
    pub current: Point,
    pub pending: Option<Point>,

    pub is_moving: bool,
}

#[derive(Default)]
enum SelectionState {
    #[default]
    Waiting,
    BeginSelection(SelectionData),
    SelectionCompleted(Rectangle),
    Abort,
}

pub struct SelectionApp {
    pub image: Box<[u8]>,
    pub buffer: Buffer,

    state: SelectionState,
}

impl SelectionApp {
    /// Returns selected region. If selection being in progress or aborted this function will
    /// return [`None`].
    pub fn selected_region(&self) -> Option<Rectangle> {
        match &self.state {
            SelectionState::SelectionCompleted(rect) => Some(rect.clone()),
            _ => None,
        }
    }
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

        let (buffer, _canvas) = partial
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
        match event.keysym {
            Keysym::Escape => {
                if let SelectionState::Waiting = self.state {
                    self.state = SelectionState::Abort;
                } else {
                    self.state = SelectionState::Waiting;
                    self.on_redraw(ctx, qh);
                }
            }

            Keysym::space => {
                if let SelectionState::BeginSelection(SelectionData { is_moving, .. }) =
                    &mut self.state
                {
                    *is_moving = true;
                }
            }

            _ => (),
        }
    }

    fn on_key_release(
        &mut self,
        _ctx: &mut WaylandContext,
        event: KeyEvent,
        _qh: &QueueHandle<WaylandApp>,
    ) {
        if event.keysym == Keysym::space {
            if let SelectionState::BeginSelection(SelectionData { is_moving, .. }) = &mut self.state
            {
                *is_moving = false;
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
        if let SelectionState::BeginSelection(SelectionData { pending, .. }) = &mut self.state {
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

        self.state = SelectionState::BeginSelection(SelectionData {
            initial: pos.clone(),
            current: pos,
            pending: None,

            is_moving: false,
        });
    }
    fn on_mouse_release(
        &mut self,
        _ctx: &mut WaylandContext,
        _pos: Point,
        _qh: &QueueHandle<WaylandApp>,
    ) {
        let SelectionState::BeginSelection(SelectionData {
            initial,
            current,
            pending: _,
            is_moving: _,
        }) = &self.state
        else {
            return;
        };

        if let Some(rect) = Rectangle::from_two_points(initial.clone(), current.clone()) {
            self.state = SelectionState::SelectionCompleted(rect);
        } else {
            // assume rectangle without area isn't a valid selection
            self.state = SelectionState::Waiting;
        }
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

        let (init, previous, pending, pending_init) = match &mut self.state {
            SelectionState::BeginSelection(SelectionData {
                initial,
                current,
                pending: pending @ Some(_),
                is_moving,
            }) if Some(current.clone()) != *pending => {
                let pending = pending.take().expect("matched");
                let prev = current.clone();
                *current = pending.clone();
                let (init, pending_init) = if *is_moving {
                    let dx = pending.x as i32 - prev.x as i32;
                    let dy = pending.y as i32 - prev.y as i32;
                    let prev_init = initial.clone();
                    let pending_init = Point::new(
                        initial.x.saturating_add_signed(dx).min(width - 1),
                        initial.y.saturating_add_signed(dy).min(height - 1),
                    );
                    *initial = pending_init.clone();
                    (prev_init, Some(pending_init))
                } else {
                    (initial.clone(), None)
                };
                (init, prev, pending, pending_init)
            }

            // Make a full-selection redraw
            SelectionState::BeginSelection(SelectionData {
                initial, current, ..
            }) if current != initial => (initial.clone(), initial.clone(), current.clone(), None),

            SelectionState::Waiting => {
                utils::dim_rect(
                    Rectangle::new(Point::new(0, 0), width - 1, height - 1),
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

        if pending_init.is_some() {
            utils::dim_crosshair(
                init.clone(),
                canvas,
                &self.image,
                width,
                height,
                Some(layer),
            );
        };

        utils::dim_crosshair(
            previous.clone(),
            canvas,
            &self.image,
            width,
            height,
            Some(layer),
        );

        utils::update_selection_partial(
            init.clone(),
            previous.clone(),
            pending.clone(),
            canvas,
            &self.image,
            width as usize,
            Some(layer),
        );

        if let Some(pending_init) = pending_init.clone() {
            utils::update_selection_partial(
                pending.clone(),
                init.clone(),
                pending_init,
                canvas,
                &self.image,
                width as usize,
                Some(layer),
            );
        }

        utils::fill_crosshair(pending_init.unwrap_or(init), canvas, width, height, Some(layer));
        utils::fill_crosshair(pending.clone(), canvas, width, height, Some(layer));

        utils::commit_drawing(layer, buffer, qh);
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

    pub fn update_selection_partial(
        init: Point,
        previous: Point,
        pending: Point,
        canvas: &mut [u8],
        image: &[u8],
        width: usize,
        layer: Option<&LayerSurface>,
    ) {
        if init.is_same_quater(&pending, &previous) {
            // NOTE: In the worst case, a double overwrite of the area (previous) -> (pending)
            // occurs here. It is assumed that the distance between these two points is small, and
            // their area is of the second-order smallness. In this case, checking for double
            // overwrite would be meaningless.

            let df_init_pending_x = init.x.abs_diff(pending.x);
            let df_init_pending_y = init.y.abs_diff(pending.y);
            let df_init_previous_x = init.x.abs_diff(previous.x);
            let df_init_previous_y = init.y.abs_diff(previous.y);

            // Dim rects
            if df_init_pending_x < df_init_previous_x {
                let proj_pending_x = Point::new(pending.x, init.y);
                if let Some(rect) = Rectangle::from_two_points(previous.clone(), proj_pending_x) {
                    dim_rect(rect, canvas, image, width, layer);
                }
            }

            if df_init_pending_y < df_init_previous_y {
                let proj_pending_y = Point::new(init.x, pending.y);
                if let Some(rect) = Rectangle::from_two_points(previous.clone(), proj_pending_y) {
                    dim_rect(rect, canvas, image, width, layer);
                }
            }

            // Copy rects
            if df_init_pending_x > df_init_previous_x {
                let proj_previous_x = Point::new(previous.x, init.y);
                if let Some(rect) = Rectangle::from_two_points(pending.clone(), proj_previous_x) {
                    copy_rect(rect, canvas, image, width, layer);
                }
            }

            if df_init_pending_y > df_init_previous_y {
                let proj_previous_y = Point::new(init.x, previous.y);
                if let Some(rect) = Rectangle::from_two_points(pending.clone(), proj_previous_y) {
                    copy_rect(rect, canvas, image, width, layer);
                }
            }
        } else {
            if let Some(rect) = Rectangle::from_two_points(init.clone(), previous.clone()) {
                dim_rect(rect, canvas, image, width, layer);
            }

            if let Some(rect) = Rectangle::from_two_points(init.clone(), pending.clone()) {
                copy_rect(rect, canvas, image, width, layer);
            }
        }
    }

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
        for row in rect.start.y..=rect.start.y + rect.height {
            let row = width * row as usize * 4;
            let start = row + rect.start.x as usize * 4;
            let end = start + (1 + rect.width) as usize * 4;
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
        for col in rect.start.x..=(rect.start.x + rect.width) {
            for row in rect.start.y..=(rect.start.y + rect.height) {
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
