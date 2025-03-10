use smithay_client_toolkit::{
    delegate_output, delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
};
use wayland_client::{globals::registry_queue_init, protocol::wl_output, Connection, QueueHandle};

pub fn output_state(conn: &Connection) -> OutputState {
    let (globals, mut event_queue) = registry_queue_init(conn).unwrap();
    let qh = event_queue.handle();

    let registry_state = RegistryState::new(&globals);
    let output_delegate = OutputState::new(&globals, &qh);

    let mut app = GetOutputs {
        registry_state,
        output_state: output_delegate,
    };

    event_queue
        .roundtrip(&mut app)
        .expect("output_state app roundtrip");

    app.output_state
}

struct GetOutputs {
    registry_state: RegistryState,
    output_state: OutputState,
}

impl OutputHandler for GetOutputs {
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

delegate_output!(GetOutputs);
delegate_registry!(GetOutputs);

impl ProvidesRegistryState for GetOutputs {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers! {
        OutputState,
    }
}
