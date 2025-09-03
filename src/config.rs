pub use envconfig::*;

#[derive(Envconfig, Debug)]
pub struct Config {
    #[envconfig(from = "DISPLAY_WIDTH", default = "25")]
    pub display_width: usize,
    #[envconfig(from = "SCROLL_INTERVAL_MS", default = "300")]
    pub scroll_interval_ms: usize,
    #[envconfig(from = "SCROLL_HOLD_INTERVALS", default = "5")]
    pub scroll_hold_intervals: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            display_width: 25,
            scroll_interval_ms: 300,
            scroll_hold_intervals: 5,
        }
    }
}
