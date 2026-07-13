pub mod app_header;
pub mod category_grid;
pub mod construction_item_card;
pub mod eco_panel;
pub mod filter_bar;
pub mod portrait_button;
pub mod queue_item_creator;
pub mod queue_item_list;
pub mod simulation_panel;
pub mod unit_block;
pub mod unit_detail;
pub mod unit_selector;
pub mod unit_selector_modal;

pub use app_header::AppHeader;
pub use category_grid::CategoryGrid;
pub use construction_item_card::ConstructionItemCard;
pub use eco_panel::EcoPanel;
pub use filter_bar::FilterBar;
pub use portrait_button::PortraitButton;
pub use queue_item_creator::QueueItemCreator;
pub use queue_item_list::QueueItemList;
pub use simulation_panel::{SimulationCommand, SimulationPanel};
pub use unit_block::UnitBlock;
pub use unit_detail::UnitDetail;
pub use unit_selector::UnitSelector;
pub use unit_selector_modal::UnitSelectorModal;

// Re-export generic UI primitives from the business-agnostic UI crate.
pub use faf_dioxus_ui::{ChartMetric, ChartTab, CountSlider, SliderField, Stat, UplotChart};
