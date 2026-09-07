use std::time::Instant;

use crate::components::time_series::{AutoYAxisTimeGraph, TimeseriesConfig};

pub struct PowerWidgetState {
    pub graph: AutoYAxisTimeGraph,
}

impl PowerWidgetState {
    pub fn init(config: TimeseriesConfig, autohide_timer: Option<Instant>) -> Self {
        PowerWidgetState {
            graph: AutoYAxisTimeGraph::new(config, autohide_timer),
        }
    }
}
