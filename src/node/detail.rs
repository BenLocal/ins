use crate::node::types::NodeRecord;

pub fn node_detail(node: &NodeRecord) -> String {
    match node {
        NodeRecord::Local() => "name: local\ntype: local".into(),
        NodeRecord::Remote(node) => format!(
            "name: {}\ntype: remote\nip: {}\nport: {}\nuser: {}\nauth: {}",
            node.name,
            node.ip,
            node.port,
            node.user,
            node.key_path
                .as_ref()
                .map(|path| format!("key:{path}"))
                .unwrap_or_else(|| {
                    if node.password.is_empty() {
                        "password:<empty>".into()
                    } else {
                        "password".into()
                    }
                })
        ),
    }
}
