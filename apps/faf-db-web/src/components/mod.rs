pub mod app_header;
pub mod blueprint_graph;
pub mod category_grid;
pub mod construction_item_card;
pub mod eco_panel;
pub mod eco_snapshot;
pub mod filter_bar;
pub mod graph_popup;
pub mod portrait_button;
pub mod queue_item_creator;
pub mod queue_item_list;
pub mod schedule_request_panel;
pub mod schedule_result_panel;
pub mod simulation_panel;
pub mod step_timeline;
pub mod unit_block;
pub mod unit_detail;
pub mod unit_selector;
pub mod unit_selector_modal;

pub use app_header::AppHeader;
pub use blueprint_graph::BlueprintGraph;
pub use category_grid::CategoryGrid;
pub use construction_item_card::ConstructionItemCard;
pub use eco_panel::EcoPanel;
pub use eco_snapshot::EcoSnapshotView;
pub use filter_bar::FilterBar;
pub use graph_popup::GraphPopup;
pub use portrait_button::PortraitButton;
pub use queue_item_creator::QueueItemCreator;
pub use queue_item_list::QueueItemList;
pub use schedule_request_panel::{ScheduleFormState, ScheduleModeTab, ScheduleRequestPanel};
pub use schedule_result_panel::ScheduleResultPanel;
pub use simulation_panel::SimulationPanel;
pub use step_timeline::StepTimeline;
pub use unit_block::UnitBlock;
pub use unit_detail::UnitDetail;
pub use unit_selector::UnitSelector;
pub use unit_selector_modal::UnitSelectorModal;

// Re-export generic UI primitives from the business-agnostic UI crate.
pub use faf_dioxus_ui::{
    ChartMetric, ChartSeries, ChartTab, CountSlider, SliderField, Stat, UplotChart,
};
