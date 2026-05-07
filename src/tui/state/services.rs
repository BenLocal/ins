pub use crate::cli::service_detail::service_detail;

use crate::store::duck::InstalledServiceRecord;

pub fn service_label(service: &InstalledServiceRecord) -> String {
    format!("{} ({})", service.service, service.app_name)
}
