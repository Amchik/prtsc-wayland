use core::cell::Cell;

use smithay_client_toolkit::shm::slot::Buffer;
use wayland_client::{globals::GlobalList, protocol::wl_shm, Connection, EventQueue, QueueHandle};
use wayland_protocols_wlr::screencopy::v1::client::{
    zwlr_screencopy_frame_v1::{self, ZwlrScreencopyFrameV1},
    zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1,
};

use super::{StatePhase, WaylandApp, WaylandAppState, WaylandAppStateFromPrevious};

pub struct ScreenshotApp {
    pub image: Option<Box<[u8]>>,
    buffer: Option<Buffer>,
    zwlr_screencopy_frame: ZwlrScreencopyFrameV1,
    buffer_format: Option<wl_shm::Format>,
}

impl WaylandAppStateFromPrevious for ScreenshotApp {
    type Previous = super::base::BaseApp;

    fn from_previous(
        ctx: &mut super::WaylandContext,
        _: Self::Previous,
        _globals: &GlobalList,
        event_queue: &mut EventQueue<WaylandApp>,
    ) -> Result<Self, super::Error> {
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

                let slot = buff.slot();
                let data = ctx
                    .partial_mut()
                    .expect("screenshot app requires at least partial state")
                    .pool
                    .raw_data_mut(&slot);
                let mut data: Vec<u8> = Vec::from(data);

                // Check for Xrgb8888 format
                // FIXME: some formats can be supported (like rgbx or rgb) but not YET implemented.
                // it is a good idea to convert here rgbx/rgb to xrgb.
                match self.buffer_format {
                    Some(wl_shm::Format::Xrgb8888) | Some(wl_shm::Format::Argb8888) => (),

                    Some(wl_shm::Format::Xbgr8888) | Some(wl_shm::Format::Abgr8888) => {
                        let cells = Cell::from_mut(&mut data[..]).as_slice_of_cells();
                        for w in cells.chunks(4) {
                            Cell::swap(&w[0], &w[2]);
                        }
                    },

                    _ => unimplemented!("Got yet unimplemented buffer format {:?}. It is a bug, please report it to github issues", self.buffer_format),
                };

                self.image = Some(data.into_boxed_slice());
            }
            _ => {}
        }
    }
}
