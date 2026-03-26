use crate::store::duck::InstalledServiceRecord;

pub fn service_label(service: &InstalledServiceRecord) -> String {
    format!("{} ({})", service.service, service.app_name)
}

pub fn service_detail(service: &InstalledServiceRecord) -> String {
    format!(
        "service: {}\napp: {}\nnode: {}\nworkspace: {}\ncreated_at_ms: {}",
        service.service,
        service.app_name,
        service.node_name,
        service.workspace,
        service.created_at_ms
    )
}
