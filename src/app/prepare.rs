use smithay_client_toolkit::{
    delegate_output, delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
};
use wayland_client::{
    globals::GlobalList,
    protocol::wl_output,
    Connection, DispatchError, EventQueue, QueueHandle,
};

use super::InitFrom;

/// Obtains outputs
#[non_exhaustive]
pub struct PrepareApp {
    pub registry_state: RegistryState,
    pub output_state: OutputState,
}

impl InitFrom<()> for PrepareApp {
    type Error = DispatchError;

    /// Register output and obtain it
    fn init(
        globals: &GlobalList,
        event_queue: &mut EventQueue<Self>,
        _: (),
    ) -> Result<Self, Self::Error> {
        let qh = event_queue.handle();

        let registry_state = RegistryState::new(globals);
        let output_state = OutputState::new(globals, &qh);

        let mut app = Self {
            registry_state,
            output_state,
        };

        event_queue.roundtrip(&mut app)?;

        Ok(app)
    }
}

impl OutputHandler for PrepareApp {
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

delegate_output!(PrepareApp);
delegate_registry!(PrepareApp);

impl ProvidesRegistryState for PrepareApp {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers! {
        OutputState,
    }
}
