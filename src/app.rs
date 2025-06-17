use base::BaseApp;
use enum_dispatch::enum_dispatch;
use screenshot::ScreenshotApp;
use selection::SelectionApp;
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
    shm::{slot::SlotPool, CreatePoolError, Shm, ShmHandler},
};
use wayland_client::{
    globals::{registry_queue_init, BindError, GlobalError, GlobalList},
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_surface},
    ConnectError, Connection, Dispatch, DispatchError, EventQueue, QueueHandle,
};
use wayland_protocols_wlr::screencopy::v1::client::{
    zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1,
    zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1,
};

use crate::points::{Point, PointInt};

pub mod base;
pub mod screenshot;
pub mod selection;

pub struct WaylandAppManager {
    globals: GlobalList,
    event_queue: EventQueue<WaylandApp>,
    qh: QueueHandle<WaylandApp>,
    pub app: WaylandApp,
}

pub struct WaylandApp {
    pub ctx: WaylandContext,
    pub state: AppState,
}

pub struct WaylandContext(WaylandContextKind);

enum WaylandContextKind {
    __Nil,

    Base(WaylandContextBase),
    Partial(WaylandContextPartial),
    Full(WaylandContextFull),
}

pub struct WaylandContextBase {
    pub registry_state: RegistryState,
    pub output_state: OutputState,
}

pub struct WaylandContextPartial {
    pub base: WaylandContextBase,

    pub shm: Shm,
    pub pool: SlotPool,

    pub logical_size: Point,
}

pub struct WaylandContextFull {
    pub partial: WaylandContextPartial,

    pub seat_state: SeatState,
    pub shape_manager: Option<CursorShapeManager>,
    pub keyboard: Option<wl_keyboard::WlKeyboard>,
    pub pointer: Option<wl_pointer::WlPointer>,

    pub layer: LayerSurface,
}

impl WaylandContext {
    pub fn base(&self) -> &WaylandContextBase {
        match &self.0 {
            WaylandContextKind::Base(v) => v,
            WaylandContextKind::Partial(v) => &v.base,
            WaylandContextKind::Full(v) => &v.partial.base,

            WaylandContextKind::__Nil => unreachable!(),
        }
    }

    pub fn base_mut(&mut self) -> &mut WaylandContextBase {
        match &mut self.0 {
            WaylandContextKind::Base(v) => v,
            WaylandContextKind::Partial(v) => &mut v.base,
            WaylandContextKind::Full(v) => &mut v.partial.base,

            WaylandContextKind::__Nil => unreachable!(),
        }
    }

    pub fn partial(&self) -> Option<&WaylandContextPartial> {
        match &self.0 {
            WaylandContextKind::Base(_) => None,
            WaylandContextKind::Partial(v) => Some(v),
            WaylandContextKind::Full(v) => Some(&v.partial),

            WaylandContextKind::__Nil => unreachable!(),
        }
    }

    pub fn partial_mut(&mut self) -> Option<&mut WaylandContextPartial> {
        match &mut self.0 {
            WaylandContextKind::Base(_) => None,
            WaylandContextKind::Partial(v) => Some(v),
            WaylandContextKind::Full(v) => Some(&mut v.partial),

            WaylandContextKind::__Nil => unreachable!(),
        }
    }

    pub fn full(&self) -> Option<&WaylandContextFull> {
        match &self.0 {
            WaylandContextKind::Full(v) => Some(v),
            _ => None,
        }
    }

    pub fn full_mut(&mut self) -> Option<&mut WaylandContextFull> {
        match &mut self.0 {
            WaylandContextKind::Full(v) => Some(v),
            _ => None,
        }
    }
}

#[enum_dispatch(WaylandAppState)]
#[allow(clippy::enum_variant_names)] // NOTE: it may removed in future and %App will renamed to %
pub enum AppState {
    BaseApp,
    ScreenshotApp,
    SelectionApp,
}

pub enum StatePhase {
    Active,
    Done,
}

#[enum_dispatch]
pub trait WaylandAppState {
    fn current_phase(&self) -> StatePhase;

    fn zwlr_screencopy_frame_event<U>(
        &mut self,
        _context: &mut WaylandContext,
        _proxy: &ZwlrScreencopyFrameV1,
        _event: <ZwlrScreencopyFrameV1 as wayland_client::Proxy>::Event,
        _data: &U,
        _conn: &Connection,
        _qh: &QueueHandle<WaylandApp>,
    ) {
    }

    fn on_mouse_enter(
        &mut self,
        _context: &mut WaylandContext,
        _pos: Point,
        _pointer: &wl_pointer::WlPointer,
        _serial: u32,
        _qh: &QueueHandle<WaylandApp>,
    ) {
    }
    fn on_mouse_move(
        &mut self,
        _context: &mut WaylandContext,
        _pos: Point,
        _qh: &QueueHandle<WaylandApp>,
    ) {
    }
    fn on_mouse_press(
        &mut self,
        _context: &mut WaylandContext,
        _pos: Point,
        _qh: &QueueHandle<WaylandApp>,
    ) {
    }
    fn on_mouse_release(
        &mut self,
        _context: &mut WaylandContext,
        _pos: Point,
        _qh: &QueueHandle<WaylandApp>,
    ) {
    }

    fn on_key_press(
        &mut self,
        _context: &mut WaylandContext,
        _event: KeyEvent,
        _qh: &QueueHandle<WaylandApp>,
    ) {
    }

    fn on_key_release(
        &mut self,
        _context: &mut WaylandContext,
        _event: KeyEvent,
        _qh: &QueueHandle<WaylandApp>,
    ) {
    }

    fn on_redraw(&mut self, _context: &mut WaylandContext, _qh: &QueueHandle<WaylandApp>) {}
}

pub trait WaylandAppStateFromPrevious: Sized {
    type Previous;

    fn from_previous(
        context: &mut WaylandContext,
        previous: Self::Previous,
        globals: &GlobalList,
        event_queue: &mut EventQueue<WaylandApp>,
    ) -> Result<Self, Error>;
}

impl WaylandAppManager {
    pub fn initialize(conn: &Connection) -> Result<Self, Error> {
        let (globals, mut event_queue) = registry_queue_init(conn).map_err(Error::Global)?;

        let qh = event_queue.handle();

        let registry_state = RegistryState::new(&globals);
        let output_state = OutputState::new(&globals, &qh);

        let mut app = WaylandApp {
            state: AppState::BaseApp(BaseApp),
            ctx: WaylandContext(WaylandContextKind::Base(WaylandContextBase {
                registry_state,
                output_state,
            })),
        };

        event_queue.roundtrip(&mut app).map_err(Error::Dispatch)?;

        Ok(Self {
            app,
            event_queue,
            globals,
            qh,
        })
    }

    pub fn initialize_partial(&mut self) -> Result<(), Error> {
        let Some(output) = self.app.ctx.base().output_state.outputs().next() else {
            return Err(Error::NoOutput);
        };

        let logical_size = {
            let Some(info) = self.app.ctx.base().output_state.info(&output) else {
                return Err(Error::NoOutputInfo);
            };

            let Some((width, height)) = info.logical_size else {
                return Err(Error::NoOutputLogicalSize);
            };

            Point::new(width as PointInt, height as PointInt)
        };

        let shm = Shm::bind(&self.globals, &self.qh).map_err(Error::Shm)?;
        let pool = SlotPool::new(logical_size.x as usize * logical_size.y as usize * 4, &shm)
            .map_err(Error::CreatePool)?;

        let WaylandContext(WaylandContextKind::Base(base)) =
            std::mem::replace(&mut self.app.ctx, WaylandContext(WaylandContextKind::__Nil))
        else {
            panic!("attempt to initialize partial context, but it have been already initialized");
        };
        self.app.ctx = WaylandContext(WaylandContextKind::Partial(WaylandContextPartial {
            base,
            logical_size,
            shm,
            pool,
        }));

        Ok(())
    }

    pub fn initialize_full(&mut self) -> Result<(), Error> {
        let seat_state = SeatState::new(&self.globals, &self.qh);
        let shape_manager = CursorShapeManager::bind(&self.globals, &self.qh).ok();

        let compositor =
            CompositorState::bind(&self.globals, &self.qh).map_err(Error::Compositor)?;
        let layer_shell = LayerShell::bind(&self.globals, &self.qh).map_err(Error::LayerShell)?;

        let surface = compositor.create_surface(&self.qh);

        let WaylandContext(WaylandContextKind::Partial(partial)) =
            std::mem::replace(&mut self.app.ctx, WaylandContext(WaylandContextKind::__Nil))
        else {
            panic!("attempt to initialize full context on non-partial context (uninitialized partial or double-initialized full)");
        };
        let size = partial.logical_size.clone();

        let layer = layer_shell.create_layer_surface(
            &self.qh,
            surface,
            Layer::Overlay,
            Some("prtsc-wayland"),
            None,
        );
        layer.set_anchor(Anchor::all());
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        layer.set_size(size.x, size.y);
        layer.commit();

        self.app.ctx = WaylandContext(WaylandContextKind::Full(WaylandContextFull {
            partial,
            seat_state,
            shape_manager,
            keyboard: None,
            pointer: None,
            layer,
        }));

        Ok(())
    }

    pub fn next_app(&mut self) -> Result<(), Error> {
        // NOTE: Since we cannot statically type the application state, the WaylandAppStateFromPrevious trait serves only
        // as a convenient interface for implementing transitions from one state to another. In
        // reality, all transitions must also be described in this function. As a bonus, this
        // function statically verifies the correctness of the transition path.

        let prev = std::mem::replace(&mut self.app.state, AppState::BaseApp(BaseApp));
        match prev {
            AppState::BaseApp(prev) => {
                self.app.state = ScreenshotApp::from_previous(
                    &mut self.app.ctx,
                    prev,
                    &self.globals,
                    &mut self.event_queue,
                )?
                .into()
            }
            AppState::ScreenshotApp(prev) => {
                self.app.state = SelectionApp::from_previous(
                    &mut self.app.ctx,
                    prev,
                    &self.globals,
                    &mut self.event_queue,
                )?
                .into()
            }
            AppState::SelectionApp(_prev) => panic!("there no next app after selection app"),
        };

        Ok(())
    }

    pub fn dispatch_until_done(&mut self) -> Result<(), Error> {
        while let StatePhase::Active = self.app.state.current_phase() {
            self.event_queue
                .blocking_dispatch(&mut self.app)
                .map_err(Error::Dispatch)?;
        }

        Ok(())
    }
}

#[derive(Debug)]
pub enum Error {
    Zwlr(BindError),
    Compositor(BindError),
    LayerShell(BindError),
    Shm(BindError),
    CreatePool(CreatePoolError),
    Global(GlobalError),
    Dispatch(DispatchError),
    Connect(ConnectError),
    NoOutput,
    NoOutputInfo,
    NoOutputLogicalSize,
}

impl<U> Dispatch<ZwlrScreencopyManagerV1, U> for WaylandApp {
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

impl<U> Dispatch<ZwlrScreencopyFrameV1, U> for WaylandApp {
    fn event(
        state: &mut Self,
        proxy: &ZwlrScreencopyFrameV1,
        event: <ZwlrScreencopyFrameV1 as wayland_client::Proxy>::Event,
        data: &U,
        conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        state
            .state
            .zwlr_screencopy_frame_event(&mut state.ctx, proxy, event, data, conn, qh);
    }
}

impl KeyboardHandler for WaylandApp {
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
        self.state.on_key_press(&mut self.ctx, event, qh);
    }

    fn release_key(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.state.on_key_release(&mut self.ctx, event, qh);
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

impl PointerHandler for WaylandApp {
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
            let Some(layer) = self.ctx.full().map(|v| &v.layer) else {
                return;
            };
            if &event.surface != layer.wl_surface() {
                continue;
            }
            let pos = Point::new(event.position.0 as PointInt, event.position.1 as PointInt);
            match event.kind {
                Enter { serial } => {
                    self.state
                        .on_mouse_enter(&mut self.ctx, pos, pointer, serial, qh);
                }
                Motion { .. } => {
                    self.state.on_mouse_move(&mut self.ctx, pos, qh);
                }
                Press { button: 272, .. } => {
                    self.state.on_mouse_press(&mut self.ctx, pos, qh);
                }
                Release { button: 272, .. } => {
                    self.state.on_mouse_release(&mut self.ctx, pos, qh);
                }
                _ => {}
            }
        }
    }
}

impl SeatHandler for WaylandApp {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self
            .ctx
            .full_mut()
            .expect("required seat_state on app without seat")
            .seat_state
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        let Some(ctx) = self.ctx.full_mut() else {
            return;
        };

        if capability == Capability::Keyboard && ctx.keyboard.is_none() {
            let keyboard = ctx
                .seat_state
                .get_keyboard(qh, &seat, None)
                .expect("Failed to create keyboard");
            ctx.keyboard = Some(keyboard);
        }

        if capability == Capability::Pointer && ctx.pointer.is_none() {
            let pointer = ctx
                .seat_state
                .get_pointer(qh, &seat)
                .expect("Failed to create pointer");
            ctx.pointer = Some(pointer);
        }
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        let Some(ctx) = self.ctx.full_mut() else {
            return;
        };

        if capability == Capability::Keyboard && ctx.keyboard.is_some() {
            ctx.keyboard.take().unwrap().release();
        }

        if capability == Capability::Pointer && ctx.pointer.is_some() {
            ctx.pointer.take().unwrap().release();
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl CompositorHandler for WaylandApp {
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
        self.state.on_redraw(&mut self.ctx, qh);
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

impl LayerShellHandler for WaylandApp {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer: &LayerSurface) {
        todo!("exit")
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        _layer: &LayerSurface,
        _configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        self.state.on_redraw(&mut self.ctx, qh);
        // idk what is that lol
    }
}

impl OutputHandler for WaylandApp {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.ctx.base_mut().output_state
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
impl ShmHandler for WaylandApp {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self
            .ctx
            .partial_mut()
            .expect("required shm_state on app without ShmState")
            .shm
    }
}

delegate_seat!(WaylandApp);
delegate_keyboard!(WaylandApp);
delegate_pointer!(WaylandApp);

delegate_output!(WaylandApp);
delegate_compositor!(WaylandApp);
delegate_shm!(WaylandApp);
delegate_layer!(WaylandApp);

delegate_registry!(WaylandApp);

impl ProvidesRegistryState for WaylandApp {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.ctx.base_mut().registry_state
    }
    registry_handlers![OutputState, SeatState];
}
