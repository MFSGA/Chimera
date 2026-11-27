// Hide the console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use chimera_widget_protocol::StatisticWidgetVariant;
use clap::Parser;

/// Shows the network statistic widget for the application that started it.
#[derive(Parser)]
struct Args {
    /// Which widget to show.
    #[arg(value_enum)]
    variant: StatisticWidgetVariant,
}

fn main() -> Result<(), chimera_widget::WidgetError> {
    chimera_widget::start_statistic_widget(Args::parse().variant)
}
