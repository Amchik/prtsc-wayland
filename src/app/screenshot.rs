use smithay_client_toolkit::{
    delegate_registry, delegate_shm,
    output::OutputState,
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shm::{
        slot::{Buffer, SlotPool},
        CreatePoolError, Shm, ShmHandler,
    },
};
use wayland_client::{
    globals::{BindError, GlobalList},
    protocol::{wl_output::WlOutput, wl_shm},
    Connection, Dispatch, DispatchError, EventQueue, QueueHandle,
};
use wayland_protocols_wlr::screencopy::v1::client::{
    zwlr_screencopy_frame_v1::{self, ZwlrScreencopyFrameV1},
    zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1,
};

use super::{
    prepare::PrepareApp, DispatchResult, DispatchWhile, InitFrom, StatePhase, WaylandApp, WaylandAppState, WaylandAppStateFromPrevious, WaylandContextPartial
};

pub struct ScreenshotApp {
    pub(super) image: Option<Box<[u8]>>,
    buffer: Option<Buffer>,
    zwlr_screencopy_frame: ZwlrScreencopyFrameV1,
    buffer_format: Option<wl_shm::Format>,
}

impl WaylandAppStateFromPrevious for ScreenshotApp {
    type Previous = super::base::BaseApp;

    fn from_previous(ctx: &mut super::WaylandContext, _: Self::Previous, _globals: &GlobalList, event_queue: &mut EventQueue<WaylandApp>) -> Result<Self, super::Error> {
        let qh = event_queue.handle();

        let Some(output) = ctx.base().output_state.outputs().next() else {
            return Err(super::Error::NoOutput);
        };

        let zwlr_screencopy_manager: ZwlrScreencopyManagerV1 = ctx
            .base()
            .registry_state
            .bind_one(&qh, 1..=3, ())
            .map_err(super::Error::Zwlr)?;

        let zwlr_screencopy_frame = zwlr_screencopy_manager.capture_output(0, &output, &qh, ());

        Ok(Self {
            image: None,
            buffer: None,
            buffer_format: None,
            zwlr_screencopy_frame,
        })
    }
}

impl ScreenshotApp {
    pub fn init(
        ctx: &mut WaylandContextPartial,
        output: &WlOutput,
        event_queue: &EventQueue<WaylandApp>,
    ) -> Result<Self, super::Error> {
        let qh = event_queue.handle();

        let zwlr_screencopy_manager: ZwlrScreencopyManagerV1 = ctx
            .base
            .registry_state
            .bind_one(&qh, 1..=3, ())
            .map_err(super::Error::Zwlr)?;

        let zwlr_screencopy_frame = zwlr_screencopy_manager.capture_output(0, output, &qh, ());

        Ok(Self {
            image: None,
            buffer: None,
            buffer_format: None,
            zwlr_screencopy_frame,
        })
    }
}

impl WaylandAppState for ScreenshotApp {
    fn current_phase(&self) -> StatePhase {
        if self.image.is_some() {
            StatePhase::Done
        } else {
            StatePhase::Active
        }
    }

    fn zwlr_screencopy_frame_event<U>(
        &mut self,
        ctx: &mut super::WaylandContext,
        _proxy: &ZwlrScreencopyFrameV1,
        event: <ZwlrScreencopyFrameV1 as wayland_client::Proxy>::Event,
        _data: &U,
        _conn: &Connection,
        _qh: &QueueHandle<WaylandApp>,
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
                    wayland_client::WEnum::Unknown(id) => {
                        panic!("`zwlr_screencopy_manager_v1` returned unsupported format: {id}")
                    }
                };
                //state.width = width;
                //state.height = height;
                self.buffer_format = Some(format);
                self.buffer = Some({
                    let (buffer, _canvas) = ctx
                        .partial_mut()
                        .expect("screenshot app requires at least partial state")
                        .pool
                        .create_buffer(width as i32, height as i32, stride as i32, format)
                        .expect("failed to create buffer");

                    self.zwlr_screencopy_frame.copy(buffer.wl_buffer());

                    buffer
                });
            }
            zwlr_screencopy_frame_v1::Event::Ready { .. } => {
                let buff = match &self.buffer {
                    Some(buffer) => buffer,
                    // another message: this piece of overengineering implemented by disabled
                    // people. please purge your windows manager and install some modern wayland
                    // compositors like sway or hyprland
                    None => {
                        panic!("`zwlr_screencopy_manager_v1` send ready event without any buffers")
                    }
                };

                // Check for Xrgb8888 format
                // FIXME: some formats can be supported (like rgbx or rgb) but not YET implemented.
                // it is a good idea to convert here rgbx/rgb to xrgb.
                let (Some(wl_shm::Format::Xrgb8888) | Some(wl_shm::Format::Argb8888)) =
                    self.buffer_format
                else {
                    unimplemented!("Got yet unimplemented buffer format {:?}. It is a bug, please report it to github issues", self.buffer_format);
                };

                let slot = buff.slot();
                let data = ctx
                    .partial_mut()
                    .expect("screenshot app requires at least partial state")
                    .pool
                    .raw_data_mut(&slot);
                self.image = Some(Box::from(data));
            }
            _ => {}
        }
    }
}

pub struct OldScreenshotApp {
    pub registry_state: RegistryState,
    pub output_state: OutputState,

    pub shm: Shm,
    pub pool: SlotPool,

    pub width: u32,
    pub height: u32,

    pub buffer: ImageBuffer,
    zwlr_screencopy_frame: ZwlrScreencopyFrameV1,
    buffer_format: Option<wl_shm::Format>,
}

/// Boxed XRGB array. Can be derefed or unwraped into [`Box<[u8]>`]
pub struct ImageBuffer(ImageBufferInner);
// Inner state
enum ImageBufferInner {
    Image(Box<[u8]>),
    Pending(Buffer),
    None,
}

impl ImageBuffer {
    pub fn unwrap(self) -> Box<[u8]> {
        match self.0 {
            ImageBufferInner::Image(v) => v,
            _ => unreachable!("returned invalid state of `app::screenshot::ImageBuffer`"),
        }
    }

    fn ready(&self) -> bool {
        matches!(self.0, ImageBufferInner::Image(_))
    }
}
impl std::ops::Deref for ImageBuffer {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        match &self.0 {
            ImageBufferInner::Image(v) => v,
            _ => unreachable!("returned invalid state of `app::screenshot::ImageBuffer`"),
        }
    }
}
impl std::ops::DerefMut for ImageBuffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match &mut self.0 {
            ImageBufferInner::Image(v) => v,
            _ => unreachable!("returned invalid state of `app::screenshot::ImageBuffer`"),
        }
    }
}

#[derive(Debug)]
pub enum Error {
    /// Failed to bind `zwlr_screencopy_manager_v1`
    Zwlr(BindError),
    Shm(BindError),
    CreatePool(CreatePoolError),
    NoOutput,
    NoOutputInfo,
    NoOutputLogicalSize,
}

impl InitFrom<PrepareApp> for OldScreenshotApp {
    type Error = Error;

    fn init(
        globals: &GlobalList,
        event_queue: &mut EventQueue<Self>,
        PrepareApp {
            registry_state,
            output_state,
            ..
        }: PrepareApp,
    ) -> Result<Self, Self::Error> {
        let qh = event_queue.handle();

        let Some(output) = output_state.outputs().next() else {
            return Err(Error::NoOutput);
        };

        let (width, height) = {
            let Some(info) = output_state.info(&output) else {
                return Err(Error::NoOutputInfo);
            };
            let Some((w, h)) = info.logical_size else {
                return Err(Error::NoOutputLogicalSize);
            };

            (w as u32, h as u32)
        };

        let shm = Shm::bind(globals, &qh).map_err(Error::Shm)?;
        let pool =
            SlotPool::new(width as usize * height as usize * 4, &shm).map_err(Error::CreatePool)?;

        let zwlr_screencopy_manager: ZwlrScreencopyManagerV1 = registry_state
            .bind_one(&qh, 1..=3, ())
            .map_err(Error::Zwlr)?;

        let zwlr_screencopy_frame = zwlr_screencopy_manager.capture_output(0, &output, &qh, ());

        Ok(Self {
            output_state,
            shm,
            registry_state,
            zwlr_screencopy_frame,
            pool,
            width,
            height,
            buffer: ImageBuffer(ImageBufferInner::None),
            buffer_format: None,
        })
    }
}

impl DispatchWhile for OldScreenshotApp {
    type Error = DispatchError;

    fn dispatch(
        &mut self,
        _globals: &GlobalList,
        event_queue: &mut EventQueue<Self>,
    ) -> Result<DispatchResult, Self::Error> {
        event_queue.blocking_dispatch(self)?;

        if self.buffer.ready() {
            Ok(DispatchResult::Done)
        } else {
            Ok(DispatchResult::Continue)
        }
    }
}

impl<U> Dispatch<ZwlrScreencopyManagerV1, U> for OldScreenshotApp {
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

impl<U> Dispatch<ZwlrScreencopyFrameV1, U> for OldScreenshotApp {
    fn event(
        state: &mut Self,
        _proxy: &ZwlrScreencopyFrameV1,
        event: <ZwlrScreencopyFrameV1 as wayland_client::Proxy>::Event,
        _data: &U,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
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
                    wayland_client::WEnum::Unknown(id) => {
                        panic!("`zwlr_screencopy_manager_v1` returned unsupported format: {id}")
                    }
                };
                state.width = width;
                state.height = height;
                state.buffer_format = Some(format);
                state.buffer = ImageBuffer(ImageBufferInner::Pending({
                    let (buffer, _canvas) = state
                        .pool
                        .create_buffer(width as i32, height as i32, stride as i32, format)
                        .expect("create buffer");

                    state.zwlr_screencopy_frame.copy(buffer.wl_buffer());

                    buffer
                }));
            }
            zwlr_screencopy_frame_v1::Event::Ready { .. } => {
                let buff = match &state.buffer.0 {
                    ImageBufferInner::Pending(buffer) => buffer,
                    // another message: this piece of overengineering implemented by disabled
                    // people. please purge your windows manager and install some modern wayland
                    // compositors like sway or hyprland
                    _ => {
                        panic!("`zwlr_screencopy_manager_v1` send ready event without any buffers")
                    }
                };

                // Check for Xrgb8888 format
                // FIXME: some formats can be supported (like rgbx or rgb) but not YET implemented.
                // it is a good idea to convert here rgbx/rgb to xrgb.
                let (Some(wl_shm::Format::Xrgb8888) | Some(wl_shm::Format::Argb8888)) =
                    state.buffer_format
                else {
                    unimplemented!("Got yet unimplemented buffer format {:?}. It is a bug, please report it to github issues", state.buffer_format);
                };

                let slot = buff.slot();
                let data = state.pool.raw_data_mut(&slot);
                state.buffer = ImageBuffer(ImageBufferInner::Image(Box::from(data)));
            }
            _ => {}
        }
    }
}

impl ShmHandler for OldScreenshotApp {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

delegate_shm!(OldScreenshotApp);
delegate_registry!(OldScreenshotApp);

impl ProvidesRegistryState for OldScreenshotApp {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![];
}
