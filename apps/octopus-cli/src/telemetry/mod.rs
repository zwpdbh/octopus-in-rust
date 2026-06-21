pub mod sink;
pub mod transport;

mod api;
pub use api::{
    EventRecord, attach_sink, disable, flush, flush_sync, get_client_info, get_or_create_device_id,
    get_sink, set_client_info, set_context, track_event, track_session_started_once,
};

// ---------------------------------------------------------------------------
// Convenience macro
// ---------------------------------------------------------------------------

#[macro_export]
macro_rules! track {
    ($event:expr) => {
        $crate::telemetry::track_event($event, ::std::default::Default::default())
    };
    ($event:expr, $($key:ident = $value:expr),* $(,)?) => {
        {
            let mut _props = ::serde_json::Map::new();
            $(
                _props.insert(::std::stringify!($key).to_string(), ::serde_json::json!($value));
            )*
            $crate::telemetry::track_event($event, _props)
        }
    };
}
