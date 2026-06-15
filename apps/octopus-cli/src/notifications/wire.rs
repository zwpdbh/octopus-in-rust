use crate::notifications::NotificationView;
use crate::wire::Notification;

/// Convert a `NotificationView` to a `Notification` wire event.
pub fn to_wire_notification(view: &NotificationView) -> Notification {
    Notification {
        id: view.event.id.clone(),
        category: view.event.category.clone(),
        notification_type: view.event.event_type.clone(),
        source_kind: view.event.source_kind.clone(),
        source_id: view.event.source_id.clone(),
        title: view.event.title.clone(),
        body: view.event.body.clone(),
        severity: view.event.severity.clone(),
        created_at: view.event.created_at,
        payload: view.event.payload.clone(),
    }
}
