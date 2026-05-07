use std::sync::Arc;

use minijinja::Environment;

const SOURCES: &[(&str, &str)] = &[
    ("layout.html", include_str!("templates/layout.html")),
    ("index.html", include_str!("templates/index.html")),
    ("error.html", include_str!("templates/error.html")),
];

pub fn build() -> Arc<Environment<'static>> {
    let mut env = Environment::new();
    for (name, src) in SOURCES {
        env.add_template(name, src)
            .expect("compile bundled template");
    }
    Arc::new(env)
}
