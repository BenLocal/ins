use std::sync::Arc;

use minijinja::Environment;

const SOURCES: &[(&str, &str)] = &[
    ("layout.html", include_str!("templates/layout.html")),
    ("index.html", include_str!("templates/index.html")),
    ("error.html", include_str!("templates/error.html")),
    ("nodes/list.html", include_str!("templates/nodes/list.html")),
    ("nodes/form.html", include_str!("templates/nodes/form.html")),
    (
        "nodes/detail.html",
        include_str!("templates/nodes/detail.html"),
    ),
    ("apps/list.html", include_str!("templates/apps/list.html")),
    ("apps/files.html", include_str!("templates/apps/files.html")),
    (
        "apps/editor.html",
        include_str!("templates/apps/editor.html"),
    ),
];

pub fn build() -> Arc<Environment<'static>> {
    let mut env = Environment::new();
    for (name, src) in SOURCES {
        env.add_template(name, src)
            .expect("compile bundled template");
    }
    Arc::new(env)
}
