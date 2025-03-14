use super::WaylandAppState;

pub struct BaseApp;

impl WaylandAppState for BaseApp {
    fn current_phase(&self) -> super::StatePhase {
        super::StatePhase::Done
    }
}
